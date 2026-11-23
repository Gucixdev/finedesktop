//! The notification center. `notts` is the store and the `ctl notify` surface; the freedesktop D-Bus front for it is
//! `session/nottsd` (a small C daemon that owns `org.freedesktop.Notifications` and calls `ctl notify send`), so Discord,
//! Telegram, Element and every `notify-send` end up here, grouped by the app that sent them.
//!
//!   ctl notify [list [APP]]            the history, newest last: `id  time  app  text`
//!   ctl notify apps                    the groups: `app  count  last time  muted?`
//!   ctl notify send [-a APP] [-u low|normal|critical] [-t TAG] [-i DBUSID] [-A key=Label,...] TEXT    post one (bare `ctl notify TEXT` does the same)
//!   ctl notify invoke ID KEY           choose one of a note's actions: the sender gets ActionInvoked (through nottsd)
//!   ctl notify dnd [on|off|toggle]     do not disturb: stored and logged, not shown, no hook (critical gets through)
//!   ctl notify snooze MINUTES|off      do not disturb until then
//!   ctl notify mute APP | unmute APP   one app silent for good
//!   ctl notify clear [APP]             forget the history (of one app)
//!   ctl notify log APP [N]             the last N lines of that app's log (default 20)
//!   ctl notify status                  dnd / snooze / mutes / missed
//! A notification shows in the bar (item `notify`) for a few seconds, is kept in a short history and fires the
//! `on-notify` hook (FD_TEXT FD_APP FD_URGENCY FD_TAG), so a script decides what else happens (sound, forwarding).
//! Every notification, quiet ones too, is appended to `$XDG_STATE_HOME/finedesktop/notts/log/APP.log`, and the
//! switches (dnd, snooze, mutes) live in `.../notts/state`, so a restart keeps them.

use std::{
    collections::{BTreeSet, VecDeque},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    components::{
        hooks::Event,
        menu::{e, head, Act, Entry},
    },
    Desktop,
};

const KEEP: usize = 200;
/// How long the bar item stays.
pub const SHOWN: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    pub fn parse(s: &str) -> Option<Urgency> {
        match s {
            "low" | "0" => Some(Urgency::Low),
            "normal" | "1" => Some(Urgency::Normal),
            "critical" | "2" => Some(Urgency::Critical),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Urgency::Low => "low",
            Urgency::Normal => "normal",
            Urgency::Critical => "critical",
        }
    }
}

pub struct Note {
    pub id: u32,
    pub at: Instant,
    pub stamp: String,
    /// the group: the app that sent it (`ctl` for a bare `ctl notify TEXT`)
    pub app: String,
    /// a free label, the D-Bus `category` hint for notifications that came over the bus (`im.received`, `email.arrived`)
    pub tag: String,
    pub urgency: Urgency,
    /// held back by do-not-disturb, a snooze or a mute: in the history, not on the bar
    pub quiet: bool,
    pub text: String,
    /// the id the sender knows it by (D-Bus Notify reply); 0 for notes that did not come over the bus
    pub nid: u32,
    /// the buttons the sender offered: (key, label)
    pub actions: Vec<(String, String)>,
}

#[derive(Default)]
pub struct Notes {
    pub list: VecDeque<Note>,
    next: u32,
    pub dnd: bool,
    /// epoch seconds, 0 = not snoozed
    pub snooze_until: u64,
    pub muted: BTreeSet<String>,
    /// quiet notes up to this id were looked at (the panel was open)
    seen: u32,
    /// where the log and the state go; none in tests
    dir: Option<PathBuf>,
}

pub fn epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `$XDG_STATE_HOME/finedesktop/notts` (default `~/.local/state/finedesktop/notts`).
pub fn state_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("finedesktop/notts"))
}

/// An app name made safe to be a file name.
pub fn slug(app: &str) -> String {
    let s: String = app.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c.to_ascii_lowercase() } else { '_' }).collect();
    let s = s.trim_matches('.').to_string();
    if s.is_empty() { "app".into() } else { s.chars().take(64).collect() }
}

impl Notes {
    /// The store with its switches read back from `dir`.
    pub fn load(dir: Option<PathBuf>) -> Notes {
        let mut n = Notes { dir, ..Notes::default() };
        if let Some(text) = n.dir.as_ref().and_then(|d| std::fs::read_to_string(d.join("state")).ok()) {
            for line in text.lines() {
                match line.split_once('=') {
                    Some(("dnd", v)) => n.dnd = v == "1",
                    Some(("snooze", v)) => n.snooze_until = v.parse().unwrap_or(0),
                    Some(("mute", v)) if !v.is_empty() => {
                        n.muted.insert(v.to_string());
                    }
                    _ => {}
                }
            }
        }
        n
    }

    fn save(&self) {
        let Some(d) = &self.dir else { return };
        let mut s = format!("dnd={}\nsnooze={}\n", self.dnd as u8, self.snooze_until);
        for m in &self.muted {
            s.push_str(&format!("mute={m}\n"));
        }
        let _ = std::fs::create_dir_all(d).and_then(|_| std::fs::write(d.join("state"), s));
    }

    pub fn snoozed(&self) -> bool {
        self.snooze_until > epoch()
    }

    /// Would a note of this app and urgency be held back right now? Critical ones always get through.
    pub fn silent_for(&self, app: &str, urgency: Urgency) -> bool {
        urgency != Urgency::Critical && (self.dnd || self.snoozed() || self.muted.contains(app))
    }

    pub fn push(&mut self, app: &str, text: &str, urgency: Urgency, tag: &str) -> (u32, bool) {
        self.push_with(app, text, urgency, tag, 0, Vec::new())
    }

    pub fn push_with(&mut self, app: &str, text: &str, urgency: Urgency, tag: &str, nid: u32, actions: Vec<(String, String)>) -> (u32, bool) {
        self.next += 1;
        let quiet = self.silent_for(app, urgency);
        if self.list.len() == KEEP {
            self.list.pop_front();
        }
        let stamp = crate::components::widgets::now("%H:%M:%S");
        self.list.push_back(Note { id: self.next, at: Instant::now(), stamp, app: app.to_string(), tag: tag.to_string(), urgency, quiet, text: text.to_string(), nid, actions });
        (self.next, quiet)
    }

    /// The newest note, if it is still within `SHOWN` and was not held back.
    pub fn current(&self) -> Option<&Note> {
        self.list.back().filter(|n| !n.quiet && n.at.elapsed() < SHOWN)
    }

    /// Quiet notes nobody looked at yet.
    pub fn missed(&self) -> usize {
        self.list.iter().filter(|n| n.quiet && n.id > self.seen).count()
    }

    pub fn mark_seen(&mut self) {
        self.seen = self.next;
    }

    /// The groups, the one with the newest note first: (app, count, time of the newest).
    pub fn apps(&self) -> Vec<(String, usize, String)> {
        let mut out: Vec<(String, usize, String)> = Vec::new();
        for n in self.list.iter().rev() {
            match out.iter_mut().find(|o| o.0 == n.app) {
                Some(o) => o.1 += 1,
                None => out.push((n.app.clone(), 1, n.stamp.clone())),
            }
        }
        for m in &self.muted {
            if !out.iter().any(|o| &o.0 == m) {
                out.push((m.clone(), 0, "-".into()));
            }
        }
        out
    }

    pub fn clear(&mut self, app: Option<&str>) -> usize {
        let before = self.list.len();
        match app {
            Some(a) => self.list.retain(|n| n.app != a),
            None => self.list.clear(),
        }
        before - self.list.len()
    }

    fn log_path(&self, app: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("log").join(format!("{}.log", slug(app))))
    }

    /// Append one line to the app's log (every note goes there, quiet or not).
    pub fn log(&self, app: &str, line: &str) {
        let Some(p) = self.log_path(app) else { return };
        let _ = p.parent().map(std::fs::create_dir_all);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn tail_log(&self, app: &str, n: usize) -> Vec<String> {
        let Some(text) = self.log_path(app).and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
    }

    pub fn status(&self) -> String {
        let snooze = if self.snoozed() { format!("{} min", (self.snooze_until - epoch()).div_ceil(60)) } else { "off".into() };
        let muted = if self.muted.is_empty() { "-".into() } else { self.muted.iter().cloned().collect::<Vec<_>>().join(",") };
        format!("dnd {}\nsnooze {snooze}\nmuted {muted}\nmissed {}\nstored {}", if self.dnd { "on" } else { "off" }, self.missed(), self.list.len())
    }
}

/// What `ctl notify send` carries.
#[derive(Debug)]
struct Send {
    app: String,
    urgency: Urgency,
    tag: String,
    text: String,
    nid: u32,
    actions: Vec<(String, String)>,
}

const SEND_USAGE: &str = "notify send [-a APP] [-u URGENCY] [-t TAG] [-i ID] [-A key=Label,...] TEXT";

/// `key=Label,key=Label` with `+` for a space and %XX for anything else (nottsd writes it, one ctl token).
fn parse_actions(s: &str) -> Result<Vec<(String, String)>, String> {
    let dec = |t: &str| -> Result<String, String> {
        let b = t.as_bytes();
        let (mut out, mut i) = (Vec::new(), 0);
        while i < b.len() {
            match b[i] {
                b'+' => out.push(b' '),
                b'%' => {
                    let h = t.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or_else(|| format!("error: bad %-escape in {t:?}"))?;
                    out.push(h);
                    i += 2;
                }
                c => out.push(c),
            }
            i += 1;
        }
        String::from_utf8(out).map_err(|_| format!("error: {t:?} is not utf-8"))
    };
    s.split(',').filter(|p| !p.is_empty()).map(|p| {
        let (k, l) = p.split_once('=').ok_or_else(|| format!("error: action {p:?} is key=Label"))?;
        Ok((dec(k)?, dec(l)?))
    }).collect()
}

/// `-a APP -u URGENCY -t TAG -i ID -A ACTIONS` in front of the text; the rest is the text.
fn parse_send(arg: &str) -> Result<Send, String> {
    let mut m = Send { app: "ctl".into(), urgency: Urgency::Normal, tag: String::new(), text: String::new(), nid: 0, actions: Vec::new() };
    let mut rest = arg.trim_start();
    while let Some(r) = rest.strip_prefix('-') {
        if r == "-" || r.starts_with("- ") {
            rest = r[1..].trim_start(); // `--` ends the flags: the text may start with a dash
            break;
        }
        let (flag, after) = r.split_once(char::is_whitespace).unwrap_or((r, ""));
        let after = after.trim_start();
        let (val, after) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
        match flag {
            "a" if !val.is_empty() => m.app = val.to_string(),
            "u" => m.urgency = Urgency::parse(val).ok_or_else(|| format!("error: urgency is low, normal or critical, not {val:?}"))?,
            "t" => m.tag = val.to_string(),
            "i" => m.nid = val.parse().map_err(|_| format!("error: -i takes the D-Bus id (a number), not {val:?}"))?,
            "A" => m.actions = parse_actions(val)?,
            _ => return Err(format!("error: unknown flag -{flag} ({SEND_USAGE})")),
        }
        rest = after.trim_start();
    }
    if rest.is_empty() {
        return Err(format!("error: nothing to say ({SEND_USAGE})"));
    }
    m.text = rest.to_string();
    Ok(m)
}

impl Desktop {
    /// Post one note: history, log, bar, hook (the last two only when it is not held back). Returns its id.
    pub fn notify_send(&mut self, app: &str, text: &str, urgency: Urgency, tag: &str) -> u32 {
        self.notify_post(app, text, urgency, tag, 0, Vec::new())
    }

    fn notify_post(&mut self, app: &str, text: &str, urgency: Urgency, tag: &str, nid: u32, actions: Vec<(String, String)>) -> u32 {
        let (id, quiet) = self.notes.push_with(app, text, urgency, tag, nid, actions);
        let stamp = crate::components::widgets::now("%Y-%m-%d %H:%M:%S");
        self.notes.log(app, &format!("{stamp}\t{}\t{}{text}", urgency.name(), if quiet { "(quiet) " } else { "" }));
        self.bar_dirty = true;
        if !quiet {
            self.emit(Event::notify(id, text, app, urgency.name(), tag));
        }
        id
    }

    /// The switches of the panel and of `ctl notify`: do-not-disturb on / off / flip.
    pub fn notify_dnd(&mut self, on: Option<bool>) {
        self.notes.dnd = on.unwrap_or(!self.notes.dnd);
        self.notes.save();
        self.bar_dirty = true;
    }

    pub fn notify_snooze(&mut self, minutes: u64) {
        self.notes.snooze_until = if minutes == 0 { 0 } else { epoch() + minutes * 60 };
        self.notes.save();
        self.bar_dirty = true;
    }

    pub fn notify_mute(&mut self, app: &str, on: bool) {
        if on {
            self.notes.muted.insert(app.to_string());
        } else {
            self.notes.muted.remove(app);
        }
        self.notes.save();
    }

    /// `notify invoke ID KEY`: tell the sender (through nottsd, which owns the bus name) that the user chose an action.
    /// The note's buttons are gone afterwards, as in every notification server.
    fn notify_invoke(&mut self, arg: &str) -> String {
        let usage = "error: notify invoke ID KEY (ids and keys: `ctl notify list`)";
        let Some((id, key)) = arg.split_once(char::is_whitespace).map(|(a, b)| (a, b.trim())) else { return usage.into() };
        let Ok(id) = id.parse::<u32>() else { return usage.into() };
        let Some(note) = self.notes.list.iter_mut().find(|n| n.id == id) else { return format!("error: no notification {id}") };
        if note.nid == 0 {
            return format!("error: notification {id} did not come over D-Bus, nobody to tell");
        }
        if !note.actions.iter().any(|(k, _)| k == key) {
            let have: Vec<&str> = note.actions.iter().map(|(k, _)| k.as_str()).collect();
            return format!("error: notification {id} has no action {key:?} (have: {})", if have.is_empty() { "none".into() } else { have.join(" ") });
        }
        let base = std::env::var("FD_NOTTS_INVOKE").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| {
            "busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.finedesktop.Notts InvokeAction us".into()
        });
        let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        crate::keys::spawn(&format!("{base} {} {}", note.nid, q(key)));
        note.actions.clear();
        self.bar_dirty = true;
        format!("ok invoked {key}")
    }

    pub fn notify_cmd(&mut self, arg: &str) -> String {
        let arg = arg.trim();
        let (word, rest) = arg.split_once(char::is_whitespace).map_or((arg, ""), |(a, b)| (a, b.trim()));
        match word {
            "" | "list" => {
                let only = (!rest.is_empty()).then_some(rest);
                let rows: Vec<String> = self.notes.list.iter().filter(|n| only.is_none_or(|a| n.app == a)).map(|n| {
                    let acts = if n.actions.is_empty() { String::new() } else { format!("\t{}", n.actions.iter().map(|(k, l)| format!("{k}={l}")).collect::<Vec<_>>().join(", ")) };
                    format!("{}\t{}\t{}\t{}{acts}", n.id, n.stamp, n.app, n.text)
                }).collect();
                if rows.is_empty() { "(no notifications)".into() } else { rows.join("\n") }
            }
            "apps" => {
                let rows: Vec<String> = self.notes.apps().into_iter().map(|(a, c, t)| format!("{a}\t{c}\t{t}{}", if self.notes.muted.contains(&a) { "\tmuted" } else { "" })).collect();
                if rows.is_empty() { "(no notifications)".into() } else { rows.join("\n") }
            }
            "clear" => {
                let n = self.notes.clear((!rest.is_empty()).then_some(rest));
                self.bar_dirty = true;
                if n == 0 && !rest.is_empty() { format!("error: no notifications from {rest}") } else { "ok".into() }
            }
            "dnd" => match rest {
                "" | "toggle" => {
                    self.notify_dnd(None);
                    format!("ok dnd {}", if self.notes.dnd { "on" } else { "off" })
                }
                "on" | "off" => {
                    self.notify_dnd(Some(rest == "on"));
                    format!("ok dnd {rest}")
                }
                _ => "error: notify dnd [on|off|toggle]".into(),
            },
            "snooze" => match rest {
                "off" | "0" => {
                    self.notify_snooze(0);
                    "ok snooze off".into()
                }
                m => match m.parse::<u64>() {
                    Ok(m) if (1..=1440).contains(&m) => {
                        self.notify_snooze(m);
                        format!("ok snooze {m} min")
                    }
                    _ => "error: notify snooze MINUTES (1..1440) | off".into(),
                },
            },
            "mute" | "unmute" if !rest.is_empty() => {
                self.notify_mute(rest, word == "mute");
                format!("ok {word} {rest}")
            }
            "mute" | "unmute" => format!("error: notify {word} APP"),
            "log" => {
                let (app, n) = rest.split_once(char::is_whitespace).map_or((rest, "20"), |(a, n)| (a, n.trim()));
                if app.is_empty() {
                    return "error: notify log APP [N]".into();
                }
                let lines = self.notes.tail_log(app, n.parse().unwrap_or(20));
                if lines.is_empty() { format!("(no log for {app})") } else { lines.join("\n") }
            }
            "status" => self.notes.status(),
            "send" => match parse_send(rest) {
                Ok(m) => format!("ok {}", self.notify_post(&m.app, &m.text, m.urgency, &m.tag, m.nid, m.actions)),
                Err(e) => e,
            },
            "invoke" => self.notify_invoke(rest),
            _ => format!("ok {}", self.notify_send("ctl", arg, Urgency::Normal, "")),
        }
    }
}

impl Desktop {
    /// Ustawienia > Powiadomienia: do-not-disturb and snooze on top, then one block per app (its newest notes,
    /// mute, clear, the log), the quiet ones marked.
    pub fn notts_entries(&self) -> Vec<Entry> {
        let n = &self.notes;
        let stay = |label: &str, line: String| e(label, Act::CtlStay(line));
        let mut v = vec![head("nie przeszkadzac: zapisane i w logu, bez paska i hooka (krytyczne przechodza)")];
        v.push(e("Nie przeszkadzac", Act::Toggle { key: "notts_dnd", on: n.dnd }));
        if n.snoozed() {
            v.push(e(&format!("Drzemka: jeszcze {} min", (n.snooze_until - epoch()).div_ceil(60)), Act::Nop));
            v.push(stay("Wylacz drzemke", "notify snooze off".into()));
        } else {
            for (label, m) in [("Drzemka 15 min", 15), ("Drzemka 1 godz.", 60), ("Drzemka 4 godz.", 240)] {
                v.push(stay(label, format!("notify snooze {m}")));
            }
        }
        if n.list.is_empty() && n.muted.is_empty() {
            v.push(head("brak powiadomien (ctl notify send -a APP TEKST; programy przez D-Bus: session/nottsd)"));
            return v;
        }
        for (app, count, last) in n.apps() {
            let muted = n.muted.contains(&app);
            v.push(head(&format!("{app}  ({count}, ostatnie {last}){}", if muted { "  [wyciszona]" } else { "" })));
            // the app's icon (its desktop file's Icon= found by the app name), when it has a PNG: before each of its notes
            let icon = crate::components::apps::icon_for(&app).filter(|i| crate::components::icons::find(i).is_some());
            for note in n.list.iter().rev().filter(|x| x.app == app).take(4) {
                let tag = if note.tag.is_empty() { String::new() } else { format!("[{}] ", note.tag) };
                let quiet = if note.quiet { "(cicho) " } else { "" };
                let mut line = e(&format!("  {}  {quiet}{tag}{}", note.stamp, note.text), Act::Nop);
                line.icon = icon.clone();
                v.push(line);
                for (key, label) in &note.actions {
                    v.push(stay(&format!("      > {label}"), format!("notify invoke {} {key}", note.id)));
                }
            }
            v.push(stay(&if muted { format!("  wlacz powiadomienia {app}") } else { format!("  wycisz {app}") }, format!("notify {} {app}", if muted { "unmute" } else { "mute" })));
            if count > 0 {
                v.push(stay(&format!("  wyczysc {app}"), format!("notify clear {app}")));
            }
            let log = state_dir().map(|d| d.join("log").join(format!("{}.log", slug(&app))));
            if let Some(p) = log.filter(|p| p.is_file()) {
                v.push(e(&format!("  log programu {app}"), Act::Run { cmd: format!("less +G '{}'", p.display()), terminal: true }));
            }
        }
        v.push(head("wszystko"));
        v.push(stay("Wyczysc cala historie", "notify clear".into()));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_bounded_history_and_the_newest_is_current() {
        let mut n = Notes::default();
        for i in 0..(KEEP + 5) {
            n.push("a", &format!("m{i}"), Urgency::Normal, "");
        }
        assert_eq!(n.list.len(), KEEP);
        assert_eq!(n.current().map(|x| x.text.as_str()), Some("m204"));
    }

    #[test]
    fn dnd_snooze_and_mute_hold_back_but_critical_gets_through() {
        let mut n = Notes::default();
        n.dnd = true;
        assert!(n.push("chat", "hi", Urgency::Normal, "").1);
        assert!(n.current().is_none(), "a quiet note is not on the bar");
        assert!(!n.push("chat", "fire", Urgency::Critical, "").1);
        assert_eq!(n.current().map(|x| x.text.as_str()), Some("fire"));
        n.dnd = false;
        n.muted.insert("spam".into());
        assert!(n.push("spam", "x", Urgency::Low, "").1);
        assert!(!n.push("mail", "y", Urgency::Low, "").1);
        n.snooze_until = epoch() + 600;
        assert!(n.push("mail", "z", Urgency::Normal, "").1);
        n.snooze_until = epoch().saturating_sub(1);
        assert!(!n.snoozed());
    }

    #[test]
    fn groups_by_app_newest_first_and_counts_missed() {
        let mut n = Notes::default();
        n.push("discord", "a", Urgency::Normal, "im.received");
        n.push("matrix", "b", Urgency::Normal, "");
        n.push("discord", "c", Urgency::Normal, "");
        let apps = n.apps();
        assert_eq!((apps[0].0.as_str(), apps[0].1), ("discord", 2));
        assert_eq!((apps[1].0.as_str(), apps[1].1), ("matrix", 1));
        n.dnd = true;
        n.push("matrix", "d", Urgency::Normal, "");
        n.push("matrix", "e", Urgency::Normal, "");
        assert_eq!(n.missed(), 2);
        n.mark_seen();
        assert_eq!(n.missed(), 0);
        assert_eq!(n.clear(Some("discord")), 2);
        assert_eq!(n.list.len(), 3);
    }

    #[test]
    fn send_flags_parse_and_reject() {
        let m = parse_send("-a telegram -u critical -t im.received  hello  world").unwrap();
        assert_eq!((m.app.as_str(), m.urgency, m.tag.as_str(), m.text.as_str()), ("telegram", Urgency::Critical, "im.received", "hello  world"));
        assert_eq!(parse_send("just text").unwrap().app, "ctl");
        assert!(parse_send("-u loud x").is_err());
        assert!(parse_send("-a x").is_err());
        assert!(parse_send("-z q text").is_err());
        let m = parse_send("-a x -- -dash and more").unwrap();
        assert_eq!((m.app.as_str(), m.text.as_str()), ("x", "-dash and more"));
    }

    #[test]
    fn actions_decode_spaces_and_escapes_and_reject_nonsense() {
        let m = parse_send("-a files -i 7 -A open=Open+folder,default=A%2BB%2C -- Done").unwrap();
        assert_eq!(m.nid, 7);
        assert_eq!(m.actions, [("open".to_string(), "Open folder".to_string()), ("default".to_string(), "A+B,".to_string())]);
        assert!(parse_send("-A nokey x").is_err());
        assert!(parse_send("-A a=%zz x").is_err());
        assert!(parse_send("-i seven x").is_err());
    }

    #[test]
    fn a_note_keeps_its_sender_id_and_buttons() {
        let mut n = Notes::default();
        n.push_with("files", "done", Urgency::Normal, "", 9, vec![("open".into(), "Open".into())]);
        let note = n.list.back().unwrap();
        assert_eq!((note.nid, note.actions.len()), (9, 1));
        assert_eq!(n.push("x", "y", Urgency::Low, "").0, 2);
        assert_eq!(n.list.back().unwrap().nid, 0);
    }

    #[test]
    fn state_and_log_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("fd-notts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut n = Notes::load(Some(dir.clone()));
        n.dnd = true;
        n.snooze_until = 4_000_000_000;
        n.muted.insert("a b".into());
        n.save();
        n.log("A b/c", "one");
        n.log("A b/c", "two");
        let m = Notes::load(Some(dir.clone()));
        assert!(m.dnd && m.snooze_until == 4_000_000_000 && m.muted.contains("a b"));
        assert_eq!(m.tail_log("A b/c", 1), vec!["two".to_string()]);
        assert_eq!(slug("A b/c"), "a_b_c");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
