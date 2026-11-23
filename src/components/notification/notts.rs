//! The notification center. `notts` is the store and the `ctl notify` surface; the freedesktop D-Bus front for it is
//! `session/nottsd` (a small C daemon that owns `org.freedesktop.Notifications` and calls `ctl notify send`), so Discord,
//! Telegram, Element and every `notify-send` end up here, grouped by the app that sent them.
//!
//!   ctl notify [list [APP]]            the history, newest last: `id  time  app  text`
//!   ctl notify apps                    the groups: `app  count  last time  muted?`
//!   ctl notify send [-a APP] [-u low|normal|critical] [-t TAG] TEXT    post one (bare `ctl notify TEXT` does the same)
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
        self.next += 1;
        let quiet = self.silent_for(app, urgency);
        if self.list.len() == KEEP {
            self.list.pop_front();
        }
        let stamp = crate::components::widgets::now("%H:%M:%S");
        self.list.push_back(Note { id: self.next, at: Instant::now(), stamp, app: app.to_string(), tag: tag.to_string(), urgency, quiet, text: text.to_string() });
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

/// `-a APP -u URGENCY -t TAG` in front of the text; the rest is the text.
fn parse_send(arg: &str) -> Result<(String, Urgency, String, String), String> {
    let (mut app, mut urg, mut tag) = ("ctl".to_string(), Urgency::Normal, String::new());
    let mut rest = arg.trim_start();
    while let Some(r) = rest.strip_prefix('-') {
        let (flag, after) = r.split_once(char::is_whitespace).unwrap_or((r, ""));
        let after = after.trim_start();
        let (val, after) = after.split_once(char::is_whitespace).unwrap_or((after, ""));
        match flag {
            "a" if !val.is_empty() => app = val.to_string(),
            "u" => urg = Urgency::parse(val).ok_or_else(|| format!("error: urgency is low, normal or critical, not {val:?}"))?,
            "t" => tag = val.to_string(),
            _ => return Err(format!("error: unknown flag -{flag} (notify send [-a APP] [-u URGENCY] [-t TAG] TEXT)")),
        }
        rest = after.trim_start();
    }
    if rest.is_empty() {
        return Err("error: nothing to say (notify send [-a APP] [-u URGENCY] [-t TAG] TEXT)".into());
    }
    Ok((app, urg, tag, rest.to_string()))
}

impl Desktop {
    /// Post one note: history, log, bar, hook (the last two only when it is not held back). Returns its id.
    pub fn notify_send(&mut self, app: &str, text: &str, urgency: Urgency, tag: &str) -> u32 {
        let (id, quiet) = self.notes.push(app, text, urgency, tag);
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

    pub fn notify_cmd(&mut self, arg: &str) -> String {
        let arg = arg.trim();
        let (word, rest) = arg.split_once(char::is_whitespace).map_or((arg, ""), |(a, b)| (a, b.trim()));
        match word {
            "" | "list" => {
                let only = (!rest.is_empty()).then_some(rest);
                let rows: Vec<String> = self.notes.list.iter().filter(|n| only.is_none_or(|a| n.app == a)).map(|n| format!("{}\t{}\t{}\t{}", n.id, n.stamp, n.app, n.text)).collect();
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
                Ok((app, urg, tag, text)) => format!("ok {}", self.notify_send(&app, &text, urg, &tag)),
                Err(e) => e,
            },
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
            for note in n.list.iter().rev().filter(|x| x.app == app).take(4) {
                let tag = if note.tag.is_empty() { String::new() } else { format!("[{}] ", note.tag) };
                let quiet = if note.quiet { "(cicho) " } else { "" };
                v.push(e(&format!("  {}  {quiet}{tag}{}", note.stamp, note.text), Act::Nop));
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
        let (a, u, t, x) = parse_send("-a telegram -u critical -t im.received  hello  world").unwrap();
        assert_eq!((a.as_str(), u, t.as_str(), x.as_str()), ("telegram", Urgency::Critical, "im.received", "hello  world"));
        assert_eq!(parse_send("just text").unwrap().0, "ctl");
        assert!(parse_send("-u loud x").is_err());
        assert!(parse_send("-a x").is_err());
        assert!(parse_send("-z q text").is_err());
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
