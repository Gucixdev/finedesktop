//! Events and hooks. Everything that happens (window opened, focus moved, workspace switched, theme or mode
//! changed, every minute ...) becomes an `Event`. An event is
//!   1. written as one text line to every `finedesktop ctl subscribe` client,
//!   2. handed to the executable `hooks/on-<name>` in the config dir (started in the background, never blocks).
//! Hooks talk back with `finedesktop ctl ...` (FD_SOCK is set), so a hook can switch theme, mode, layout, spawn...
//! Environment of a hook: FD_EVENT FD_LINE FD_SOCKET and, where they make sense, FD_WINDOW FD_APPID FD_TITLE
//! FD_OUTPUT FD_WORKSPACE FD_LAYOUT FD_THEME FD_MODE FD_NAME FD_TEXT FD_TIME.

use std::{io::Write, os::unix::fs::PermissionsExt, process::{Command, Stdio}};

use crate::state::Desktop;

pub struct Event {
    pub kind: &'static str,
    pub line: String,
    pub env: Vec<(&'static str, String)>,
}

impl Event {
    fn new(kind: &'static str, line: String, env: Vec<(&'static str, String)>) -> Event {
        Event { kind, line, env }
    }
    pub fn start() -> Event {
        Event::new("start", "start".into(), vec![])
    }
    pub fn reload() -> Event {
        Event::new("reload", "reload".into(), vec![])
    }
    pub fn open(win: u32, output: &str, ws: usize, app: &str, title: &str) -> Event {
        Event::new("open", format!("open {win} {output} {ws} {app} {title}"), vec![
            ("FD_WINDOW", win.to_string()), ("FD_OUTPUT", output.into()), ("FD_WORKSPACE", ws.to_string()),
            ("FD_APPID", app.into()), ("FD_TITLE", title.into()),
        ])
    }
    pub fn close(win: u32, app: &str, title: &str) -> Event {
        Event::new("close", format!("close {win} {app} {title}"), vec![
            ("FD_WINDOW", win.to_string()), ("FD_APPID", app.into()), ("FD_TITLE", title.into()),
        ])
    }
    pub fn focus(win: u32, app: &str, title: &str) -> Event {
        Event::new("focus", format!("focus {win} {app} {title}"), vec![
            ("FD_WINDOW", win.to_string()), ("FD_APPID", app.into()), ("FD_TITLE", title.into()),
        ])
    }
    pub fn title(win: u32, app: &str, title: &str) -> Event {
        Event::new("title", format!("title {win} {app} {title}"), vec![
            ("FD_WINDOW", win.to_string()), ("FD_APPID", app.into()), ("FD_TITLE", title.into()),
        ])
    }
    pub fn tag(output: &str, ws: usize, occupied: u32) -> Event {
        Event::new("tag", format!("tag {output} {ws} {occupied}"), vec![
            ("FD_OUTPUT", output.into()), ("FD_WORKSPACE", ws.to_string()), ("FD_OCCUPIED", occupied.to_string()),
        ])
    }
    pub fn layout(output: &str, name: &str) -> Event {
        Event::new("layout", format!("layout {output} {name}"), vec![("FD_OUTPUT", output.into()), ("FD_LAYOUT", name.into())])
    }
    pub fn output(name: &str) -> Event {
        Event::new("output", format!("output {name}"), vec![("FD_OUTPUT", name.into())])
    }
    pub fn theme(name: &str) -> Event {
        Event::new("theme", format!("theme {name}"), vec![("FD_THEME", name.into())])
    }
    pub fn mode(name: &str) -> Event {
        Event::new("mode", format!("mode {name}"), vec![("FD_MODE", name.into())])
    }
    pub fn user(name: &str, text: &str) -> Event {
        Event::new("user", format!("user {name} {text}"), vec![("FD_NAME", name.into()), ("FD_TEXT", text.into())])
    }
    pub fn minute(hhmm: &str) -> Event {
        Event::new("minute", format!("minute {hhmm}"), vec![("FD_TIME", hhmm.into())])
    }

    /// File name of the hook that reacts to this event.
    pub fn hook_name(&self) -> &'static str {
        match self.kind {
            "start" => "on-start",
            "open" => "on-window-open",
            "close" => "on-window-close",
            "focus" => "on-focus",
            "title" => "on-title-change",
            "tag" => "on-tag-change",
            "layout" => "on-layout-change",
            "output" => "on-output",
            "theme" => "on-theme-change",
            "mode" => "on-mode-change",
            "reload" => "on-reload",
            "user" => "on-user",
            _ => "on-minute",
        }
    }
}

pub const HOOKS: [&str; 13] = [
    "on-start", "on-window-open", "on-window-close", "on-focus", "on-title-change", "on-tag-change",
    "on-layout-change", "on-output", "on-theme-change", "on-mode-change", "on-reload", "on-user", "on-minute",
];

const MODES: [(&str, &str); 5] = [
    ("nightmode", include_str!("../.finedesktop/mode/nightmode")),
    ("daymode", include_str!("../.finedesktop/mode/daymode")),
    ("gamemode", include_str!("../.finedesktop/mode/gamemode")),
    ("ecomode", include_str!("../.finedesktop/mode/ecomode")),
    ("afkmode", include_str!("../.finedesktop/mode/afkmode")),
];

pub fn find_hook(name: &str) -> Option<std::path::PathBuf> {
    let p = crate::config::config_dir()?.join("hooks").join(name);
    let md = std::fs::metadata(&p).ok()?;
    (md.is_file() && md.permissions().mode() & 0o111 != 0).then_some(p)
}

fn run_shell(cmd: &str) {
    if cmd.trim().is_empty() {
        return;
    }
    crate::keys::spawn(cmd);
}

impl Desktop {
    /// Publish an event: subscribers get the line, the matching hook gets started, the bar redraws.
    pub fn emit(&mut self, ev: Event) {
        self.bar_dirty = true;
        let line = format!("{}\n", ev.line.replace('\n', " "));
        self.subs.retain_mut(|s| s.write_all(line.as_bytes()).is_ok());
        let Some(path) = find_hook(ev.hook_name()) else { return };
        let mut cmd = Command::new(path);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd.env("FD_EVENT", ev.kind).env("FD_LINE", &ev.line);
        if let Some(sock) = std::env::var_os("FD_SOCK") {
            cmd.env("FD_SOCKET", sock);
        }
        for (k, v) in &ev.env {
            cmd.env(k, v);
        }
        match cmd.spawn() {
            // reap in the background so finished hooks do not stay zombies
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => tracing::warn!("hook {}: {e}", ev.hook_name()),
        }
    }

    pub fn mode_names() -> Vec<String> {
        let mut v: Vec<String> = MODES.iter().map(|(n, _)| n.to_string()).collect();
        if let Some(dir) = crate::config::config_dir() {
            if let Ok(rd) = std::fs::read_dir(dir.join("mode")) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_string();
                    if !n.starts_with('.') && !v.contains(&n) {
                        v.push(n);
                    }
                }
            }
        }
        v
    }

    fn mode_text(name: &str) -> Option<String> {
        if name.contains('/') || name.starts_with('.') {
            return None;
        }
        if let Some(dir) = crate::config::config_dir() {
            if let Ok(t) = std::fs::read_to_string(dir.join("mode").join(name)) {
                return Some(t);
            }
        }
        MODES.iter().find(|(n, _)| *n == name).map(|(_, t)| t.to_string())
    }

    /// `mode NAME` / `mode off` / `mode list`. A mode changes how the system behaves, never how it looks.
    pub fn set_mode(&mut self, name: &str) -> String {
        match name {
            "list" => return Desktop::mode_names().join(" "),
            "off" | "none" => return self.leave_mode(),
            _ => {}
        }
        let Some(text) = Desktop::mode_text(name) else {
            return format!("error: unknown mode {name:?} (have: {})", Desktop::mode_names().join(" "));
        };
        // switching straight from one mode to another leaves the first one properly
        if self.mode.is_some() {
            self.leave_mode();
        }
        let saved = self.config.clone();
        let mut next = self.config.clone();
        // a mode file is ordinary config lines: behaviour by default, looks (theme ...) only if the file asks for it
        let errs = next.apply(&text);
        if let Some(e) = errs.into_iter().next() {
            return format!("error: mode {name}: {e}");
        }
        self.config = next;
        let enter = self.config.enter.clone();
        let after = self.config.theme.name.clone();
        self.mode = Some((name.to_string(), Box::new(saved), after));
        self.apply_widget_state();
        run_shell(&enter);
        self.emit(Event::mode(name));
        "ok".into()
    }

    fn leave_mode(&mut self) -> String {
        let Some((name, saved, theme_after_enter)) = self.mode.take() else { return "ok (no mode active)".into() };
        let leave = self.config.leave.clone();
        let current_theme = self.config.theme.name.clone();
        self.config = *saved;
        // the theme goes back only if the mode itself changed it; a theme picked meanwhile stays
        if current_theme != theme_after_enter {
            let _ = self.config.set_theme(&current_theme);
        }
        self.apply_widget_state();
        self.retile();
        run_shell(&leave);
        self.emit(Event::mode("off"));
        tracing::info!("left mode {name}");
        "ok".into()
    }

    /// `theme NAME` / `theme list`.
    pub fn set_theme(&mut self, name: &str) -> String {
        if name == "list" {
            return Theme::list_line(&self.config.theme.name);
        }
        match self.config.set_theme(name) {
            Ok(()) => {
                self.retile();
                self.emit(Event::theme(name));
                "ok".into()
            }
            Err(e) => format!("error: {e}"),
        }
    }
}

use crate::components::theme::Theme;

impl Theme {
    /// "zen matrix* ..." with the active one starred.
    pub fn list_line(active: &str) -> String {
        Theme::list().iter().map(|n| if n == active { format!("{n}*") } else { n.clone() }).collect::<Vec<_>>().join(" ")
    }
}
