//! Windows come back after a restart. Wayland cannot hand live windows to a new compositor process, so the desktop
//! keeps a list instead: which app, its command, where it sat (monitor, workspace), its tags and tab name. On the
//! next start the apps whose windows are not there yet are started again and each new window is put back where its
//! predecessor was. The list is one tab-separated text file per name, `<state dir>/finedesktop/sessions/NAME.tsv`:
//!   mon  ws  app_id  name  tag,tag  cwd  command
//! `last` is kept up to date by itself (once a second, only when something changed, and on exit).
//!   ctl session save [NAME]      write the list now          ctl session restore [NAME]   start what is missing
//!   ctl session saved            the names that exist         ctl session restart          save, exit 75, the session
//!                                                             script starts the (possibly newer) binary again
//! Config: `restore = on|off` (default on) decides whether `last` is restored at start. What comes back is the app
//! and its place, not its contents: an app brings its own back (browser tabs, an editor's session). The command is
//! read from /proc of the window's client; when that program does not exist on the host (a Flatpak sandbox shows
//! `/app/...`) the app's .desktop entry is used instead.

use std::path::{Path, PathBuf};

use crate::{components::apps, flows::lifecycle::Component, Desktop};

pub const EXIT_RESTART: i32 = 75;
const DELAY: u32 = 6; // seconds after start, so `exec =` lines and the autostart folder have already run
const PENDING_FOR: u32 = 90; // seconds a saved place stays reserved for its app

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub mon: usize,
    pub ws: usize,
    pub app: String,
    pub name: String,
    pub tags: Vec<String>,
    pub cwd: String,
    pub cmd: String,
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\t', "\\t").replace('\n', "\\n")
}

fn unesc(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

pub fn render(rows: &[Row]) -> String {
    rows.iter()
        .map(|r| format!("{}\t{}\t{}\t{}\t{}\t{}\t{}\n", r.mon, r.ws, esc(&r.app), esc(&r.name), esc(&r.tags.join(",")), esc(&r.cwd), esc(&r.cmd)))
        .collect()
}

/// Lines that do not parse are skipped: a hand-edited file must not take the desktop down.
pub fn parse(text: &str) -> Vec<Row> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(7, '\t').collect();
            if f.len() != 7 {
                return None;
            }
            Some(Row {
                mon: f[0].parse().ok()?,
                ws: f[1].parse().ok()?,
                app: unesc(f[2]),
                name: unesc(f[3]),
                tags: unesc(f[4]).split(',').filter(|t| crate::flows::tags::valid(t)).map(String::from).collect(),
                cwd: unesc(f[5]),
                cmd: unesc(f[6]),
            })
        })
        .collect()
}

pub fn quote(a: &str) -> String {
    format!("'{}'", a.replace('\'', "'\\''"))
}

/// /proc/PID/cmdline: NUL-separated, a trailing NUL ends the last one.
pub fn args(raw: &[u8]) -> Vec<String> {
    let mut v: Vec<String> = raw.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// The socket of the terminal sessions of `fd-shell` (session/fd-shell): its own tmux server, apart from the user's tmux.
const TMUX_SOCKET: &str = "finedesktop";

/// The session name in the command line of an `fd-shell` tmux client (`tmux -L finedesktop ... new-session -A -s NAME`);
/// None for any other program, including the user's own tmux on the default socket.
pub fn tmux_name(argv: &[String]) -> Option<String> {
    let prog = argv.first()?;
    if Path::new(prog).file_name()? != "tmux" {
        return None;
    }
    let after = |flag: &str| argv.iter().position(|a| a == flag).and_then(|i| argv.get(i + 1));
    (after("-L")? == TMUX_SOCKET).then_some(())?;
    after("-s").filter(|n| valid_name(n)).cloned()
}

/// The tmux session behind a terminal window: the `fd-shell` client somewhere below the window's process.
pub fn tmux_of_pid(pid: i32) -> Option<String> {
    let mut todo = vec![pid];
    let mut seen = Vec::new();
    while let Some(p) = todo.pop() {
        if seen.contains(&p) || seen.len() > 64 {
            continue;
        }
        seen.push(p);
        if let Some(n) = std::fs::read(format!("/proc/{p}/cmdline")).ok().and_then(|raw| tmux_name(&args(&raw))) {
            return Some(n);
        }
        for task in std::fs::read_dir(format!("/proc/{p}/task")).into_iter().flatten().flatten() {
            if let Ok(kids) = std::fs::read_to_string(task.path().join("children")) {
                todo.extend(kids.split_whitespace().filter_map(|k| k.parse::<i32>().ok()));
            }
        }
    }
    None
}

fn program_exists(p: &str) -> bool {
    if p.contains('/') {
        return Path::new(p).is_file();
    }
    std::env::var("PATH").unwrap_or_default().split(':').any(|d| !d.is_empty() && Path::new(d).join(p).is_file())
}

/// The command to start the app again: its own command line when that program runs on this host, else the
/// desktop entry's Exec (the Flatpak case), else nothing.
pub fn command(argv: &[String], entry: Option<&str>) -> Option<String> {
    match argv.first() {
        Some(p) if program_exists(p) => Some(argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")),
        _ => entry.filter(|e| !e.is_empty()).map(String::from),
    }
}

/// The saved rows whose app has no window yet. Each present window covers one row of its app.
pub fn missing(rows: &[Row], present: &[String]) -> Vec<Row> {
    let mut have: Vec<String> = present.iter().map(|a| a.to_lowercase()).collect();
    rows.iter()
        .filter(|r| match have.iter().position(|a| *a == r.app.to_lowercase()) {
            Some(i) => {
                have.remove(i);
                false
            }
            None => true,
        })
        .cloned()
        .collect()
}

/// The first reserved place for `app`, removed from the list.
pub fn take(pending: &mut Vec<Row>, app: &str) -> Option<Row> {
    let i = pending.iter().position(|r| r.app.eq_ignore_ascii_case(app))?;
    Some(pending.remove(i))
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 48 && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !n.starts_with('.')
}

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("finedesktop/sessions"))
}

fn file(name: &str) -> Option<PathBuf> {
    valid_name(name).then(|| dir().map(|d| d.join(format!("{name}.tsv")))).flatten()
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let parent = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let tmp = path.with_extension("tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    f.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

impl Desktop {
    fn client_pid(&self, w: &smithay::desktop::Window) -> Option<i32> {
        let t = w.toplevel()?;
        let client = smithay::reexports::wayland_server::Resource::client(t.wl_surface())?;
        client.get_credentials(&self.display_handle).ok().map(|c| c.pid)
    }

    /// The tmux session of every window that runs in one (terminal windows started through `fd-shell`).
    pub fn tmux_names(&self) -> Vec<String> {
        self.mons.iter().flat_map(|m| m.ws.iter()).flat_map(|ws| ws.clients.iter()).filter_map(|w| self.client_pid(w)).filter_map(tmux_of_pid).collect()
    }

    /// A terminal window is being closed on purpose: its session ends with it (a restart, by contrast, keeps it).
    pub fn tmux_kill_for(&self, w: &smithay::desktop::Window) {
        if let Some(name) = self.client_pid(w).and_then(tmux_of_pid) {
            crate::keys::spawn(&format!("tmux -L {TMUX_SOCKET} kill-session -t {}", quote(&name)));
        }
    }

    /// Every window with an app id, in monitor / workspace / stacking order. `full` also reads the command and
    /// working folder (cheap place-only rows are enough to notice a change).
    pub fn session_rows(&self, full: bool) -> Vec<Row> {
        let mut entries: Option<Vec<apps::App>> = None;
        let mut rows = Vec::new();
        for (mi, m) in self.mons.iter().enumerate() {
            for (wi, ws) in m.ws.iter().enumerate() {
                for w in &ws.clients {
                    let (id, app, _) = Desktop::win_info(w);
                    if app.is_empty() {
                        continue;
                    }
                    let mut r = Row { mon: mi, ws: wi, app, name: self.names.get(&id).cloned().unwrap_or_default(), tags: self.tags.of(id), ..Row::default() };
                    if full {
                        let pid = self.client_pid(w);
                        let argv = pid.and_then(|p| std::fs::read(format!("/proc/{p}/cmdline")).ok()).map(|raw| args(&raw)).unwrap_or_default();
                        let entry = entries.get_or_insert_with(apps::scan).iter().find(|a| a.id.to_lowercase() == format!("{}.desktop", r.app.to_lowercase())).map(|a| a.exec.clone());
                        r.cmd = command(&argv, entry.as_deref()).unwrap_or_default();
                        // a terminal in a tmux session comes back by attaching to it: what ran there is still running
                        if let (Some(name), Some(prog)) = (pid.and_then(tmux_of_pid), argv.first()) {
                            r.cmd = format!("{} -e fd-shell {}", quote(prog), quote(&name));
                        }
                        r.cwd = pid.and_then(|p| std::fs::read_link(format!("/proc/{p}/cwd")).ok()).map(|p| p.display().to_string()).unwrap_or_default();
                    }
                    rows.push(r);
                }
            }
        }
        rows
    }

    pub fn session_save(&mut self, name: &str) -> Result<usize, String> {
        let path = file(name).ok_or_else(|| format!("bad session name {name:?} (letters, digits, - _ .)"))?;
        let rows: Vec<Row> = self.session_rows(true);
        write(&path, &render(&rows))?;
        Ok(rows.len())
    }

    /// Start the apps of `name` that have no window yet and reserve their places for the windows they open.
    pub fn session_restore(&mut self, name: &str) -> Result<usize, String> {
        let path = file(name).ok_or_else(|| format!("bad session name {name:?}"))?;
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let present: Vec<String> = self.session_rows(false).into_iter().map(|r| r.app).collect();
        let todo = missing(&parse(&text), &present);
        let mut started = 0;
        for r in &todo {
            if r.cmd.is_empty() {
                tracing::warn!("restore: no command for {}", r.app);
                continue;
            }
            let cd = if r.cwd.is_empty() { String::new() } else { format!("cd {} 2>/dev/null; ", quote(&r.cwd)) };
            crate::keys::spawn(&format!("{cd}{}", r.cmd));
            self.pending.push(r.clone());
            started += 1;
        }
        Ok(started)
    }

    pub fn session_saved(&self) -> String {
        let mut v: Vec<String> = dir().and_then(|d| std::fs::read_dir(d).ok()).into_iter().flatten().flatten().filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|_| e.path().extension().is_some_and(|x| x == "tsv"))).collect();
        v.sort();
        if v.is_empty() { "(nothing saved)".into() } else { v.join(" ") }
    }

    /// `ctl session save|restore|saved|restart`; None when the word is not one of ours.
    pub fn restore_cmd(&mut self, what: &str) -> Option<String> {
        let (verb, name) = what.split_once(char::is_whitespace).map(|(v, n)| (v, n.trim())).unwrap_or((what, ""));
        let name = if name.is_empty() { "last" } else { name };
        Some(match verb {
            "save" => match self.session_save(name) {
                Ok(n) => format!("saved {n} windows as {name}"),
                Err(e) => format!("error: {e}"),
            },
            "restore" => match self.session_restore(name) {
                Ok(n) => format!("started {n} apps from {name}"),
                Err(e) => format!("error: {e}"),
            },
            "saved" => self.session_saved(),
            "restart" => {
                if let Err(e) = self.session_save("last") {
                    return Some(format!("error: not restarting, {e}"));
                }
                RESTART.store(true, std::sync::atomic::Ordering::Relaxed);
                self.loop_signal.stop();
                "restarting".into()
            }
            _ => return None,
        })
    }
}

/// Set by `ctl session restart`; main exits with EXIT_RESTART when it is.
pub static RESTART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct Restore {
    ticks: u32,
    restored_at: Option<u32>,
    last: String,
    seen_windows: bool,
}

impl Restore {
    fn save_if_changed(&mut self, d: &mut Desktop) {
        // the tmux names are part of the signature: a terminal's session appears a moment after its window does
        let sig = render(&d.session_rows(false)) + &d.tmux_names().join(",");
        if sig == self.last {
            return;
        }
        self.seen_windows |= !sig.is_empty();
        self.last = sig;
        // an empty list never replaces a saved one until this run has had windows: a fresh start must not erase it
        if self.seen_windows {
            if let Err(e) = d.session_save("last") {
                tracing::warn!("session save: {e}");
            }
        }
    }
}

impl Component for Restore {
    fn name(&self) -> &'static str {
        "restore"
    }
    fn tick(&mut self, d: &mut Desktop) {
        self.ticks += 1;
        if self.ticks == DELAY && d.config.restore {
            match d.session_restore("last") {
                Ok(n) => tracing::info!("restore: started {n} apps"),
                Err(e) => tracing::info!("restore: {e}"),
            }
            self.restored_at = Some(self.ticks);
        }
        if self.restored_at.is_some_and(|t| self.ticks > t + PENDING_FOR) && !d.pending.is_empty() {
            d.pending.clear();
        }
        // before the restore ran the desktop is empty on purpose; saving then would overwrite what it is about to read
        if self.ticks >= DELAY {
            self.save_if_changed(d);
        }
    }
    fn destroy(&mut self, d: &mut Desktop) {
        if self.ticks >= DELAY && !d.session_rows(false).is_empty() {
            let _ = d.session_save("last");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(app: &str) -> Row {
        Row { mon: 0, ws: 1, app: app.into(), name: "web".into(), tags: vec!["a".into(), "b".into()], cwd: "/home/x".into(), cmd: "'foot' '-e' 'frost'".into() }
    }

    #[test]
    fn rows_survive_the_file_even_with_tabs_newlines_and_backslashes() {
        let mut r = row("org.x.App");
        r.name = "a\tb\nc\\d".into();
        r.cmd = "echo 'a\tb'".into();
        let text = render(&[r.clone(), row("foot")]);
        assert_eq!(text.lines().count(), 2, "one line per window");
        assert_eq!(parse(&text), vec![r, row("foot")]);
    }

    #[test]
    fn bad_lines_are_skipped_not_fatal() {
        let text = format!("garbage\nx\ty\n{}", render(&[row("foot")]));
        assert_eq!(parse(&text), vec![row("foot")]);
        assert!(parse("").is_empty());
    }

    #[test]
    fn command_lines_are_quoted_and_a_trailing_nul_is_not_an_argument() {
        assert_eq!(args(b"foot\0-e\0my prog\0"), vec!["foot", "-e", "my prog"]);
        assert_eq!(quote("it's"), "'it'\\''s'");
        let sh = vec!["sh".to_string(), "-c".into(), "echo 'hi there'".into()];
        let cmd = command(&sh, Some("ignored")).unwrap();
        assert_eq!(cmd, "'sh' '-c' 'echo '\\''hi there'\\'''");
    }

    #[test]
    fn a_program_missing_on_the_host_falls_back_to_the_desktop_entry() {
        let sandbox = vec!["/app/lib/firefox/firefox".to_string()];
        assert_eq!(command(&sandbox, Some("/usr/bin/flatpak run org.mozilla.firefox")).as_deref(), Some("/usr/bin/flatpak run org.mozilla.firefox"));
        assert_eq!(command(&sandbox, None), None);
        assert_eq!(command(&[], Some("")), None);
    }

    #[test]
    fn only_the_fd_shell_tmux_counts() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(tmux_name(&a("tmux -L finedesktop -f /x/tmux.conf new-session -A -s fd-12-3")).as_deref(), Some("fd-12-3"));
        assert_eq!(tmux_name(&a("/usr/bin/tmux -L finedesktop new-session -A -s work")).as_deref(), Some("work"));
        assert_eq!(tmux_name(&a("tmux new-session -A -s work")), None, "the user's own tmux is not ours");
        assert_eq!(tmux_name(&a("tmux -L other new -s x")), None);
        assert_eq!(tmux_name(&a("foot -s x")), None);
        assert_eq!(tmux_name(&a("tmux -L finedesktop new -s a/b")), None);
        assert_eq!(tmux_name(&[]), None);
    }

    #[test]
    fn a_present_window_covers_one_row_of_its_app() {
        let rows = vec![row("foot"), row("foot"), row("Firefox")];
        let left = missing(&rows, &["FOOT".to_string()]);
        assert_eq!(left.iter().map(|r| r.app.as_str()).collect::<Vec<_>>(), vec!["foot", "Firefox"]);
        assert_eq!(missing(&rows, &[]).len(), 3);
    }

    #[test]
    fn places_are_handed_out_once_per_window_case_insensitively() {
        let mut p = vec![row("foot"), row("foot")];
        assert!(take(&mut p, "FOOT").is_some() && take(&mut p, "foot").is_some());
        assert!(take(&mut p, "foot").is_none());
    }

    #[test]
    fn session_names_cannot_escape_the_folder() {
        assert!(valid_name("last") && valid_name("work-2.v1"));
        assert!(!valid_name("") && !valid_name("../x") && !valid_name("a/b") && !valid_name(".hidden") && !valid_name("a b"));
    }
}
