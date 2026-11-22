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
    /// false: bus only, no `hooks/on-*` script (per-component lifecycle, per-frame events)
    pub scripts: bool,
}

impl Event {
    fn new(kind: &'static str, line: String, env: Vec<(&'static str, String)>) -> Event {
        Event { kind, line, env, scripts: true }
    }
    fn bus_only(mut self) -> Event {
        self.scripts = false;
        self
    }
    /// A component's own `awake` / `start` / `restart` / `destroy` (scripts hear the desktop's, see lifecycle.rs).
    pub fn life(kind: &'static str, name: &str) -> Event {
        Event::new(kind, format!("{kind} {name}"), vec![("FD_NAME", name.into())]).bus_only()
    }
    pub fn restart() -> Event {
        Event::new("restart", "restart desktop".into(), vec![("FD_NAME", "desktop".into())])
    }
    pub fn destroy() -> Event {
        Event::new("destroy", "destroy desktop".into(), vec![("FD_NAME", "desktop".into())])
    }
    pub fn tick() -> Event {
        Event::new("tick", "tick".into(), vec![])
    }
    /// `update` / `fixupdate`: a line with the time step, never a script.
    pub fn hot(kind: &'static str, dt: f32) -> Event {
        Event::new(kind, format!("{kind} {dt:.4}"), vec![]).bus_only()
    }
    pub fn toggled(on: bool, name: &str) -> Event {
        let kind = if on { "enabled" } else { "disabled" };
        Event::new(kind, format!("{kind} {name}"), vec![("FD_NAME", name.into())])
    }
    pub fn change(name: &str, old: &str, new: &str) -> Event {
        Event::new("change", format!("change {name} {new}"), vec![("FD_NAME", name.into()), ("FD_VALUE", new.into()), ("FD_OLD", old.into())])
    }
    pub fn tagged(win: u32, tag: &str, on: bool) -> Event {
        Event::new("tagged", format!("tagged {win} {} {tag}", if on { '+' } else { '-' }), vec![
            ("FD_WINDOW", win.to_string()), ("FD_NAME", tag.into()), ("FD_VALUE", if on { "+" } else { "-" }.into()),
        ])
    }
    pub fn hook(name: &str) -> Event {
        Event::new("hook", format!("hook {name}"), vec![("FD_NAME", name.into())])
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
    pub fn notify(id: u32, text: &str) -> Event {
        Event::new("notify", format!("notify {id} {text}"), vec![("FD_NOTE", id.to_string()), ("FD_TEXT", text.into())])
    }
    pub fn minute(hhmm: &str) -> Event {
        Event::new("minute", format!("minute {hhmm}"), vec![("FD_TIME", hhmm.into())])
    }

    /// File name of the hook that reacts to this event.
    pub fn hook_name(&self) -> &'static str {
        match self.kind {
            "restart" => "on-restart",
            "destroy" => "on-destroy",
            "tick" => "on-tick",
            "enabled" => "on-enabled",
            "disabled" => "on-disabled",
            "change" => "on-change",
            "tagged" => "on-tagged",
            "hook" => "on-hook",
            "minute" => "on-minute",
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
            "notify" => "on-notify",
            _ => "",
        }
    }
}

/// Every script name. `on-event` also runs for every ordinary event (FD_EVENT says which); `on-hook` after any other hook ran.
pub const HOOKS: [&str; 24] = [
    "on-start", "on-restart", "on-destroy", "on-tick", "on-enabled", "on-disabled", "on-window-open", "on-window-close",
    "on-focus", "on-title-change", "on-tag-change", "on-layout-change", "on-output", "on-theme-change", "on-mode-change",
    "on-reload", "on-user", "on-notify", "on-minute", "on-change", "on-tagged", "on-hook", "on-event", "on-awake",
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
    /// Publish an event: stream and call subscribers on the bus get it, then the scripts (see flows/bus.rs).
    pub fn emit(&mut self, ev: Event) {
        use crate::flows::bus::{self, Sink};
        let hot = bus::is_hot(ev.kind);
        if !hot {
            self.bar_dirty = true;
        }
        let line = format!("{}\n", ev.line.replace('\n', " "));
        self.bus.subs.retain_mut(|s| match &mut s.sink {
            Sink::Stream(st) if bus::wants(&s.topics, ev.kind) => st.write_all(line.as_bytes()).is_ok(),
            _ => true,
        });
        // in-process listeners are taken out while they run: they cannot hear what they cause, and may subscribe more
        let (mut calls, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.bus.subs).into_iter().partition(|s| matches!(s.sink, Sink::Call(_)));
        self.bus.subs = keep;
        for s in calls.iter_mut() {
            if let Sink::Call(f) = &mut s.sink {
                if bus::wants(&s.topics, ev.kind) {
                    f(self, &ev);
                }
            }
        }
        self.bus.subs.append(&mut calls);
        if !ev.scripts || matches!(ev.kind, "update" | "fixupdate") {
            return;
        }
        let mut ran = Vec::new();
        if self.run_hook(ev.hook_name(), &ev) {
            ran.push(ev.hook_name());
        }
        if !hot && self.run_hook("on-event", &ev) {
            ran.push("on-event");
        }
        // `hook` is never announced for itself, so a script on-hook cannot start itself forever
        if ev.kind != "hook" {
            for name in ran {
                self.emit(Event::hook(name));
            }
        }
    }

    /// Start `hooks/NAME` in the background if it exists and is executable. True when it was started.
    fn run_hook(&self, name: &str, ev: &Event) -> bool {
        if name.is_empty() {
            return false;
        }
        let Some(path) = find_hook(name) else { return false };
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
                true
            }
            Err(e) => {
                tracing::warn!("hook {name}: {e}");
                false
            }
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
