//! Config: one tiny line format shared by the built-in defaults, the user file and the IPC
//! socket (actions). No dependencies, every error is reported with its line number.

use smithay::input::keyboard::xkb;

use crate::components::engine::layout_engine::Layout;

pub const DEFAULT: &str = include_str!("default.conf");
pub const STEAMDECK: &str = include_str!("steamdeck.conf");

/// Steam Deck (DMI product Jupiter = LCD, Galileo = OLED) or `FD_PROFILE=steamdeck`.
pub fn is_steam_deck() -> bool {
    if let Ok(p) = std::env::var("FD_PROFILE") {
        return p == "steamdeck";
    }
    std::fs::read_to_string("/sys/class/dmi/id/product_name").is_ok_and(|n| matches!(n.trim(), "Jupiter" | "Galileo"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Mods {
    pub logo: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Next,
    Prev,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutSel {
    Set(Layout),
    Dir(Dir),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsSel {
    N(usize),
    Dir(Dir),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Spawn(String),
    Close,
    Quit,
    Reload,
    ReloadEngine,
    Layouts,
    /// `ctl menu ...`; also what the keys do while the menu is open
    Menu(String),
    /// `ctl prefer [ROLE [COMMAND]]`
    Prefer(String),
    Wallpaper(String),
    Focus(Dir),
    /// `focus left|right|up|down`: the window in that direction, or the monitor beyond it
    FocusDir(crate::components::engine::focus_engine::Cardinal),
    /// `move left|right|up|down`: swap with the window in that direction, or send to the monitor beyond it
    MoveDir(crate::components::engine::focus_engine::Cardinal),
    /// `focus last`: the window focused before this one
    FocusLast,
    Zoom,
    Layout(LayoutSel),
    Mfact(f32),
    Nmaster(i32),
    View(WsSel),
    Tag(WsSel),
    FocusMon(Dir),
    SendMon(Dir),
    /// true: as JSON
    State(bool),
    /// `session lock|logout|suspend|hibernate|reboot|poweroff`
    Session(String),
    Autostart(String),
    Tags(String),
    Group(String),
    Panels(String),
    Layers,
    Components,
    Enable(String),
    Disable(String),
    Restart,
    Observe(String),
    Power(String),
    Crash(String),
    /// `trash PATH | list | restore NAME`
    Trash(String),
    /// `notify [TEXT | list | clear]`
    Notify(String),
    /// `name [WINDOW] TEXT`: the tab/stack label of a window (default: the focused one); `-` restores its title
    Name(String),
    /// `log [N]`: the last N log lines
    Log(String),
    Vt(u8),
    /// `theme NAME`, `theme list`
    Theme(String),
    /// `mode NAME`, `mode off`, `mode list`
    Mode(String),
    /// `emit NAME TEXT`: a user event for hooks and subscribers
    Emit(String),
    /// `bar toggle|top|bottom|off`
    Bar(String),
    Panel(String),
    Screenshot(String),
    Screenrecord(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarPos {
    Top,
    Bottom,
    Off,
}

/// `segment = NAME SECONDS COMMAND`: a bar widget that runs COMMAND every SECONDS and shows its first line.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub name: String,
    pub every: u64,
    pub cmd: String,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub mods: Mods,
    pub sym: u32,
    pub action: Action,
    /// as typed, for the key panel: "Super+Return" and "spawn foot"
    pub label: String,
    pub act: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub modkey: Mods,
    pub terminal: String,
    /// command of `ctl session lock`
    pub lock: String,
    /// window rules (`rule = APP ws=N name=TEXT`)
    pub rules: Vec<crate::components::window::Rule>,
    /// warn at this battery percentage (0 = off)
    pub low_battery: i32,
    /// filesystem the `disk` / `disk-bar` bar items show (any path on it)
    pub disk: String,
    /// actions refused when they come over `ctl`
    pub ctl_deny: Vec<String>,
    /// preferred browser, editor, files (the terminal is `terminal`): see settings_preferaps
    pub prefer: std::collections::BTreeMap<String, String>,
    pub menu: String,
    pub gap: i32,
    pub mfact: f32,
    pub nmaster: usize,
    pub layout: Layout,
    pub workspaces: usize,
    pub background: [f32; 4],
    pub binds: Vec<Binding>,
    pub exec: Vec<String>,
    pub theme: crate::components::theme::Theme,
    pub bar: BarPos,
    pub bar_left: Vec<String>,
    pub bar_right: Vec<String>,
    pub segments: Vec<Segment>,
    /// seconds between bar refreshes (clock, cpu, ram ...)
    pub bar_interval: u64,
    /// strftime formats of the `date` and `clock`/`time` bar widgets (`%H:%M` = no seconds)
    pub clock_format: String,
    pub date_format: String,
    pub font_scale: i32,
    /// `anim_speed`: 0.25..4, 1 = the default springs, 2 = twice as fast
    pub anim_speed: f32,
    /// `anim_bounce`: 0..1 overshoot of the springs (0.5 = the default small one)
    pub anim_bounce: f32,
    /// `output = NAME mode=WxH@HZ pos=X,Y off`: per connector, read by the DRM backend at start
    pub outputs: Vec<crate::components::outputs::OutputCfg>,
    /// `kb_repeat_rate` (keys per second) and `kb_repeat_delay` (ms before repeating)
    pub kb_repeat_rate: i32,
    pub kb_repeat_delay: i32,
    /// `pointer_speed` -1..1 (0 = the device default), `natural_scroll`, `tap_to_click`: applied to every libinput device
    pub pointer_speed: f32,
    pub natural_scroll: bool,
    pub tap_to_click: bool,
    /// `accel_profile = flat|adaptive`, `left_handed`, `middle_emulation`, `disable_while_typing`: libinput, every device that has them
    pub accel_flat: bool,
    pub left_handed: bool,
    pub middle_emulation: bool,
    pub disable_while_typing: bool,
    /// picture shown by `swaybg` behind the windows (empty: the theme's background colour)
    pub wallpaper: String,
    /// `wallpaper_mode = fill|fit|stretch|center|tile`
    pub wallpaper_mode: crate::components::ast::Mode,
    /// `wallpaper_OUTPUT = PATH`: pictures that replace the default one on one output
    pub wallpaper_outputs: Vec<(String, String)>,
    /// `font = PATH`: a .bdf or .psf bitmap font; empty or unreadable = the built-in Terminus 8x16
    pub font: String,
    /// height of the tab/stack bar in px; 0 = one text row plus 2 (slimmer than the status bar)
    pub tab_height: i32,
    /// background widget scripts (`segment`s and widgets/ files) run; modes switch this off
    pub widgets: bool,
    /// the window under the pointer takes keyboard focus when the pointer moves onto it
    pub focus_follows_mouse: bool,
    /// the pointer jumps to the window that gets focus by key (it stays when it is already inside it)
    pub mouse_follows_focus: bool,
    /// `tearing = on`: page flips do not wait for vblank (lower latency, visible tearing); DRM only, falls back when the driver refuses
    pub tearing: bool,
    /// `restore = off`: do not start the apps of the last session again at start (components/restore.rs)
    pub restore: bool,
    /// path of libfinedesktop_engine.so (hot-reloadable layout/focus engines); empty = the default below
    pub engine: String,
    /// xkb layout list and options, e.g. `kb_layout = pl,gb` and `kb_options = grp:shifts_toggle`
    pub kb_layout: String,
    pub kb_options: String,
    /// shell commands a mode runs when it is entered / left (mode files only)
    pub enter: String,
    pub leave: String,
    /// panels drawn on the desktop (key list + markdown files)
    pub panels: bool,
    pub panel_keys: bool,
    pub panel_files: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            modkey: Mods { logo: true, ..Mods::default() },
            terminal: "foot".into(),
            lock: "loginctl lock-session".into(),
            ctl_deny: Vec::new(),
            rules: Vec::new(),
            low_battery: 15,
            disk: "/".into(),
            prefer: crate::components::settings::settings_preferaps::defaults(),
            menu: "dmenu_run".into(),
            gap: 0,
            mfact: 0.55,
            nmaster: 1,
            layout: Layout::Tile,
            workspaces: 9,
            background: [0.0, 0.0, 0.0, 1.0],
            binds: Vec::new(),
            exec: Vec::new(),
            theme: Default::default(),
            bar: BarPos::Top,
            bar_left: vec!["tags".into(), "layout".into(), "title".into()],
            bar_right: vec!["cpu".into(), "ram".into(), "vol".into(), "bat".into(), "clock".into()],
            segments: Vec::new(),
            bar_interval: 1,
            clock_format: "%H:%M:%S".into(),
            date_format: "%Y-%m-%d".into(),
            font_scale: 1,
            anim_speed: 1.0,
            anim_bounce: 0.5,
            outputs: Vec::new(),
            kb_repeat_rate: 25,
            kb_repeat_delay: 200,
            pointer_speed: 0.0,
            natural_scroll: false,
            tap_to_click: false,
            accel_flat: false,
            left_handed: false,
            middle_emulation: false,
            disable_while_typing: true,
            wallpaper: String::new(),
            wallpaper_mode: crate::components::ast::Mode::Fill,
            wallpaper_outputs: Vec::new(),
            font: String::new(),
            tab_height: 0,
            widgets: true,
            focus_follows_mouse: true,
            mouse_follows_focus: true,
            tearing: false,
            restore: true,
            engine: String::new(),
            kb_layout: String::new(),
            kb_options: String::new(),
            enter: String::new(),
            leave: String::new(),
            panels: true,
            panel_keys: true,
            panel_files: Vec::new(),
        }
    }
}

fn norm(sym: u32) -> u32 {
    if (0x41..=0x5a).contains(&sym) { sym + 0x20 } else { sym }
}

pub fn keysym(name: &str) -> Option<u32> {
    let s = xkb::keysym_from_name(name, xkb::KEYSYM_CASE_INSENSITIVE);
    (s.raw() != 0).then(|| norm(s.raw()))
}

fn parse_dir(s: &str) -> Result<Dir, String> {
    match s {
        "next" => Ok(Dir::Next),
        "prev" => Ok(Dir::Prev),
        _ => Err(format!("expected next|prev, got {s:?}")),
    }
}

fn parse_ws(s: &str) -> Result<WsSel, String> {
    if let Ok(n) = s.parse::<usize>() {
        return if n >= 1 { Ok(WsSel::N(n)) } else { Err("workspaces start at 1".into()) };
    }
    parse_dir(s).map(WsSel::Dir)
}

pub fn parse_action(line: &str) -> Result<Action, String> {
    let line = line.trim();
    let (cmd, rest) = line.split_once(char::is_whitespace).map_or((line, ""), |(a, b)| (a, b.trim()));
    let num = |s: &str| s.trim_start_matches('+').parse::<f32>().map_err(|_| format!("not a number: {s:?}"));
    Ok(match cmd {
        "spawn" if !rest.is_empty() => Action::Spawn(rest.into()),
        "close" => Action::Close,
        "quit" => Action::Quit,
        "reload" => Action::Reload,
        "reload-engine" => Action::ReloadEngine,
        "layouts" => Action::Layouts,
        "prefer" => Action::Prefer(rest.into()),
        "wallpaper" => Action::Wallpaper(rest.into()),
        "menu" => Action::Menu(if rest.is_empty() { "toggle".into() } else { rest.into() }),
        "state" => Action::State(rest == "json"),
        "session" => Action::Session(rest.into()),
        "autostart" => Action::Autostart(rest.into()),
        "tags" => Action::Tags(rest.into()),
        "group" => Action::Group(rest.into()),
        "panels" => Action::Panels(rest.into()),
        "layers" => Action::Layers,
        "components" => Action::Components,
        "enable" if !rest.is_empty() => Action::Enable(rest.into()),
        "disable" if !rest.is_empty() => Action::Disable(rest.into()),
        "restart" => Action::Restart,
        "observe" => Action::Observe(rest.into()),
        "power" => Action::Power(rest.into()),
        "crash" => Action::Crash(rest.into()),
        "trash" => Action::Trash(rest.into()),
        "notify" => Action::Notify(rest.into()),
        "name" if !rest.is_empty() => Action::Name(rest.into()),
        "log" => Action::Log(rest.into()),
        "screenshot" => Action::Screenshot(rest.into()),
        "screenrecord" => Action::Screenrecord(rest.into()),
        "zoom" => Action::Zoom,
        "theme" if !rest.is_empty() => Action::Theme(rest.into()),
        "mode" if !rest.is_empty() => Action::Mode(rest.into()),
        "emit" if !rest.is_empty() => Action::Emit(rest.into()),
        "bar" if !rest.is_empty() => Action::Bar(rest.into()),
        "welcome" | "panel" => Action::Panel(if rest.is_empty() { "toggle".into() } else { rest.into() }),
        "focus" => match crate::components::engine::focus_engine::Cardinal::parse(rest) {
            Some(c) => Action::FocusDir(c),
            None if rest == "last" => Action::FocusLast,
            None => Action::Focus(parse_dir(rest)?),
        },
        "move" => Action::MoveDir(
            crate::components::engine::focus_engine::Cardinal::parse(rest).ok_or("move left|right|up|down")?,
        ),
        "layout" => match rest {
            "next" | "prev" => Action::Layout(LayoutSel::Dir(parse_dir(rest)?)),
            name => Action::Layout(LayoutSel::Set(
                Layout::from_name(name).ok_or_else(|| format!("unknown layout {name:?}"))?,
            )),
        },
        "mfact" => Action::Mfact(num(rest)?),
        "nmaster" => Action::Nmaster(num(rest)? as i32),
        "view" => Action::View(parse_ws(rest)?),
        "tag" => Action::Tag(parse_ws(rest)?),
        "monitor" => match rest.split_once(' ') {
            Some(("focus", d)) => Action::FocusMon(parse_dir(d.trim())?),
            Some(("send", d)) => Action::SendMon(parse_dir(d.trim())?),
            _ => return Err("monitor focus|send next|prev".into()),
        },
        "" => return Err("empty action".into()),
        other => return Err(format!("unknown action {other:?}")),
    })
}

impl Config {
    pub fn parse_mods(&self, s: &str) -> Result<Mods, String> {
        let mut m = Mods::default();
        for tok in s.split('+').map(str::trim).filter(|t| !t.is_empty()) {
            match tok.to_ascii_lowercase().as_str() {
                "mod" => {
                    m.logo |= self.modkey.logo;
                    m.alt |= self.modkey.alt;
                    m.ctrl |= self.modkey.ctrl;
                    m.shift |= self.modkey.shift;
                }
                "super" | "logo" => m.logo = true,
                "alt" => m.alt = true,
                "ctrl" | "control" => m.ctrl = true,
                "shift" => m.shift = true,
                "none" => {}
                other => return Err(format!("unknown modifier {other:?}")),
            }
        }
        Ok(m)
    }

    /// Apply `text` on top of the current values. Returns one message per bad line.
    pub fn apply(&mut self, text: &str) -> Vec<String> {
        let mut errs = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split(" # ").next().unwrap_or(raw).trim(); // inline comment needs "space # space", so #rrggbb survives
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Err(e) = self.apply_line(line) {
                errs.push(format!("line {}: {e}", i + 1));
            }
        }
        errs
    }

    fn apply_line(&mut self, line: &str) -> Result<(), String> {
        let (key, val) = line.split_once('=').ok_or("expected `key = value`")?;
        let (key, val) = (key.trim(), val.trim());
        let f = |v: &str| v.parse::<f32>().map_err(|_| format!("not a number: {v:?}"));
        let n = |v: &str| v.parse::<i32>().map_err(|_| format!("not an integer: {v:?}"));
        match key {
            "mod" => self.modkey = self.parse_mods(val).map(|m| if m == Mods::default() { self.modkey } else { m })?,
            "terminal" => self.terminal = val.into(),
            "lock" => self.lock = val.into(),
            "rule" => self.rules.push(crate::components::window::Rule::parse(val).ok_or_else(|| format!("rule: APP [ws=N] [name=TEXT], got {val:?}"))?),
            "disk" => self.disk = expand_home(val),
            "low_battery" => self.low_battery = n(val)?.clamp(0, 100),
            "ctl_deny" => self.ctl_deny = val.split_whitespace().map(String::from).collect(),
            "menu" => self.menu = expand_home(val),
            "gap" => self.gap = n(val)?.max(0),
            "mfact" => self.mfact = f(val)?.clamp(0.1, 0.9),
            "nmaster" => self.nmaster = n(val)?.max(0) as usize,
            "workspaces" => self.workspaces = (n(val)?.clamp(1, 32)) as usize,
            "layout" => self.layout = Layout::from_name(val).ok_or_else(|| format!("unknown layout {val:?}"))?,
            "background" => self.background = parse_color(val)?,
            "theme" => self.set_theme(val)?,
            "border" => self.theme.border = n(val)?.max(0),
            "animations" => self.theme.animations = matches!(val, "on" | "true" | "yes" | "1"),
            "bar" => self.bar = match val {
                "top" | "on" => BarPos::Top,
                "bottom" => BarPos::Bottom,
                "off" => BarPos::Off,
                _ => return Err(format!("bar = top|bottom|off, got {val:?}")),
            },
            "bar_left" => self.bar_left = val.split_whitespace().map(String::from).collect(),
            "bar_right" => self.bar_right = val.split_whitespace().map(String::from).collect(),
            "clock_format" if !val.is_empty() => self.clock_format = val.to_string(),
            "date_format" if !val.is_empty() => self.date_format = val.to_string(),
            "bar_interval" => self.bar_interval = n(val)?.clamp(1, 3600) as u64,
            "font" => self.font = expand_home(val),
            "font_scale" => self.font_scale = n(val)?.clamp(1, 4),
            "output" => {
                let c = crate::components::outputs::OutputCfg::parse(val)?;
                self.outputs.retain(|o| o.name != c.name);
                self.outputs.push(c);
            }
            "kb_repeat_rate" => self.kb_repeat_rate = n(val)?.clamp(1, 100),
            "kb_repeat_delay" => self.kb_repeat_delay = n(val)?.clamp(100, 1000),
            "pointer_speed" => self.pointer_speed = f(val)?.clamp(-1.0, 1.0),
            "natural_scroll" => self.natural_scroll = matches!(val, "on" | "true" | "yes" | "1"),
            "tap_to_click" => self.tap_to_click = matches!(val, "on" | "true" | "yes" | "1"),
            "accel_profile" => self.accel_flat = match val { "flat" => true, "adaptive" => false, _ => return Err("accel_profile is flat or adaptive".into()) },
            "left_handed" => self.left_handed = matches!(val, "on" | "true" | "yes" | "1"),
            "middle_emulation" => self.middle_emulation = matches!(val, "on" | "true" | "yes" | "1"),
            "disable_while_typing" => self.disable_while_typing = matches!(val, "on" | "true" | "yes" | "1"),
            "wallpaper" => self.wallpaper = expand_home(val),
            "wallpaper_mode" => self.wallpaper_mode = crate::components::ast::Mode::parse(val)?,
            k if k.starts_with("wallpaper_") => {
                let out = &k["wallpaper_".len()..];
                self.wallpaper_outputs.retain(|(o, _)| o != out);
                if !val.is_empty() {
                    self.wallpaper_outputs.push((out.to_string(), expand_home(val)));
                }
            }
            "anim_speed" => self.anim_speed = f(val)?.clamp(0.25, 4.0),
            "anim_bounce" => self.anim_bounce = f(val)?.clamp(0.0, 1.0),
            "tab_height" => self.tab_height = n(val)?.clamp(0, 200),
            "welcome_auto" => self.panels = matches!(val, "on" | "true" | "yes" | "1"),
            "welcome_keys" => self.panel_keys = matches!(val, "on" | "true" | "yes" | "1"),
            "welcome" => self.panel_files.push(val.into()),
            "kb_layout" => self.kb_layout = val.into(),
            "kb_options" => self.kb_options = val.into(),
            "engine" => self.engine = val.to_string(),
            "mouse_follows_focus" => self.mouse_follows_focus = matches!(val, "on" | "true" | "yes" | "1"),
            "restore" => self.restore = matches!(val, "on" | "true" | "yes" | "1"),
            "tearing" => self.tearing = matches!(val, "on" | "true" | "yes" | "1"),
            "focus_follows_mouse" => self.focus_follows_mouse = matches!(val, "on" | "true" | "yes" | "1"),
            "widgets" => self.widgets = matches!(val, "on" | "true" | "yes" | "1"),
            "enter" => self.enter = val.into(),
            "leave" => self.leave = val.into(),
            "segment" => {
                let mut p = val.splitn(3, char::is_whitespace);
                let (name, every, cmd) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or("").trim());
                let every = every.parse::<u64>().map_err(|_| "segment = NAME SECONDS COMMAND".to_string())?;
                if name.is_empty() || cmd.is_empty() {
                    return Err("segment = NAME SECONDS COMMAND".into());
                }
                self.segments.retain(|s| s.name != name);
                self.segments.push(Segment { name: name.into(), every: every.max(1), cmd: cmd.into() });
            }
            "exec" => self.exec.push(val.into()),
            "bind" => {
                let mut p = val.splitn(3, ',').map(str::trim);
                let (mods, k, act) = (p.next().unwrap_or(""), p.next().ok_or("bind = MODS, key, action")?, p.next().ok_or("bind = MODS, key, action")?);
                let mods = self.parse_mods(mods)?;
                let sym = keysym(k).ok_or_else(|| format!("unknown key {k:?}"))?;
                let act = act.to_string();
                // the roles first: their commands may mention $terminal (the generic editor default does)
                let act = self.prefer.iter().fold(act, |a, (role, cmd)| a.replace(&format!("${role}"), cmd));
                let act = act.replace("$terminal", &self.terminal.clone()).replace("$menu", &self.menu.clone());
                let action = parse_action(&act)?;
                self.binds.retain(|b| !(b.mods == mods && b.sym == sym));
                let mut label: Vec<&str> = Vec::new();
                for (on, n) in [(mods.logo, "Super"), (mods.ctrl, "Ctrl"), (mods.alt, "Alt"), (mods.shift, "Shift")] {
                    if on {
                        label.push(n);
                    }
                }
                label.push(k);
                self.binds.push(Binding { mods, sym, action, label: label.join("+"), act: act.clone() });
            }
            "unbind" => {
                let (mods, k) = val.split_once(',').ok_or("unbind = MODS, key")?;
                let (mods, sym) = (self.parse_mods(mods)?, keysym(k.trim()).ok_or("unknown key")?);
                self.binds.retain(|b| !(b.mods == mods && b.sym == sym));
            }
            other if other.starts_with("prefer_") => self.prefer_set(&other["prefer_".len()..], val)?,
            other => return Err(format!("unknown setting {other:?}")),
        }
        Ok(())
    }

    /// The command of a preferred-application role (`terminal` is the `terminal` setting).
    pub fn prefer_get(&self, role: &str) -> Option<String> {
        if role == "terminal" { Some(self.terminal.clone()) } else { self.prefer.get(role).cloned() }
    }

    pub fn prefer_set(&mut self, role: &str, cmd: &str) -> Result<(), String> {
        if cmd.is_empty() {
            return Err(format!("prefer_{role} needs a command"));
        }
        match role {
            "terminal" => self.terminal = cmd.into(),
            r if crate::components::settings::settings_preferaps::ROLES.contains(&r) => {
                self.prefer.insert(r.into(), cmd.into());
            }
            _ => return Err(format!("unknown role {role:?} (have: {})", crate::components::settings::settings_preferaps::ROLES.join(" "))),
        }
        Ok(())
    }

    /// Defaults, then the user file. Errors are logged, never fatal.
    pub fn load() -> Config {
        let mut c = Config::default();
        for e in c.apply(DEFAULT) {
            tracing::error!("built-in config: {e}");
        }
        if is_steam_deck() {
            for e in c.apply(STEAMDECK) {
                tracing::error!("steamdeck preset: {e}");
            }
        }
        if let Some(path) = user_path() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                for e in c.apply(&text) {
                    tracing::warn!("{}: {e}", path.display());
                }
            }
        }
        smithay::backend::drm::TEARING.store(c.tearing, std::sync::atomic::Ordering::Relaxed);
        // the font is process-wide state: a path that cannot be read leaves the built-in one in place
        if let Err(e) = crate::components::font::Font::select(&c.font) {
            tracing::warn!("{e}");
            let _ = crate::components::font::Font::select("");
        }
        c
    }

    /// Switch the theme and copy its looks into the live settings.
    /// Height of the status bar and of the tab/stack bars: one text row plus padding.
    pub fn bar_height(&self) -> i32 {
        crate::components::font::Font::current().h * self.font_scale + 6
    }

    /// Height of the tab / stack bars: a few pixels slimmer than the status bar unless `tab_height` says otherwise.
    pub fn tab_bar_height(&self) -> i32 {
        if self.tab_height > 0 { self.tab_height } else { crate::components::font::Font::current().h * self.font_scale + 2 }
    }

    pub fn modkey_label(&self) -> &'static str {
        let m = self.modkey;
        if m.logo { "Super" } else if m.alt { "Alt" } else if m.ctrl { "Ctrl" } else { "Shift" }
    }

    pub fn set_theme(&mut self, name: &str) -> Result<(), String> {
        let t = crate::components::theme::Theme::load(name)?;
        self.gap = t.gap;
        self.background = crate::components::theme::to_f32(t.bg);
        self.theme = t;
        Ok(())
    }

    pub fn find(&self, mods: Mods, sym: u32) -> Option<&Action> {
        let sym = norm(sym);
        self.binds.iter().find(|b| b.mods == mods && b.sym == sym).map(|b| &b.action)
    }
}

/// `$XDG_CONFIG_HOME/finedesktop` (or `$FD_CONFIG_DIR`): config, themes/, hooks/, widgets/, mode/, autostart/.
/// Where the engine library is looked for: `engine = PATH`, else ~/.local/lib/finedesktop/libfinedesktop_engine.so.
pub fn engine_path(c: &Config) -> Option<std::path::PathBuf> {
    if !c.engine.is_empty() {
        return Some(std::path::PathBuf::from(c.engine.replacen('~', &std::env::var("HOME").unwrap_or_default(), 1)));
    }
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/lib/finedesktop/libfinedesktop_engine.so"))
}

pub fn config_dir() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("FD_CONFIG_DIR") {
        return Some(d.into());
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("finedesktop"))
}

pub fn user_path() -> Option<std::path::PathBuf> {
    config_dir().map(|d| d.join("config"))
}

fn expand_home(s: &str) -> String {
    match (s.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(h)) => format!("{h}/{rest}"),
        _ => s.into(),
    }
}

fn parse_color(s: &str) -> Result<[f32; 4], String> {
    let h = s.strip_prefix('#').ok_or("colour is #rrggbb")?;
    if h.len() != 6 {
        return Err("colour is #rrggbb".into());
    }
    let ch = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map(|v| v as f32 / 255.0).map_err(|_| "bad hex".to_string());
    Ok([ch(0)?, ch(2)?, ch(4)?, 1.0])
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_bar_has_workspaces_left_and_status_right() {
        let mut c = Config::default();
        assert!(c.apply(DEFAULT).is_empty());
        assert_eq!(c.bar_left, vec!["tags", "layout"]);
        assert_eq!(c.bar_right.first().map(String::as_str), Some("notify"));
        assert_eq!(c.bar_right.last().map(String::as_str), Some("time"));
    }

    use super::*;

    #[test]
    fn wallpaper_keys_parse_per_output_and_an_empty_value_removes_it() {
        let mut c = Config::default();
        assert!(c.apply("wallpaper = /a.png\nwallpaper_mode = tile\nwallpaper_DP-1 = /b.png\nwallpaper_HDMI-A-1 = /c.png\nwallpaper_DP-1 =").is_empty());
        assert_eq!(c.wallpaper_mode, crate::components::ast::Mode::Tile);
        assert_eq!(c.wallpaper_outputs, [("HDMI-A-1".to_string(), "/c.png".to_string())]);
        assert!(!c.apply("wallpaper_mode = zoom").is_empty());
    }

    #[test]
    fn steamdeck_preset_parses_and_sets_the_list_layout() {
        let mut c = Config::default();
        assert!(c.apply(DEFAULT).is_empty());
        let errs = c.apply(STEAMDECK);
        assert!(errs.is_empty(), "{errs:?}");
        assert_eq!(c.layout, Layout::List);
        assert_eq!(c.font_scale, 2);
        assert_eq!(c.workspaces, 4);
    }

    #[test]
    fn mouse_follows_focus_is_on_and_tearing_off_by_default_and_both_parse() {
        let mut c = default_cfg();
        assert!(c.mouse_follows_focus && !c.tearing);
        assert!(c.apply("mouse_follows_focus = off\ntearing = on").is_empty());
        assert!(!c.mouse_follows_focus && c.tearing);
        assert_eq!(c.clock_format, "%H:%M:%S");
        assert!(c.apply("clock_format = %H:%M").is_empty());
        assert_eq!(c.clock_format, "%H:%M");
        assert_eq!(c.font, "~/.local/share/fonts/Fixed6x13.bdf".replacen("~", &std::env::var("HOME").unwrap_or_default(), 1));
    }

    fn default_cfg() -> Config {
        let mut c = Config::default();
        assert_eq!(c.apply(DEFAULT), Vec::<String>::new());
        c
    }

    #[test]
    fn defaults_parse_cleanly_and_have_binds() {
        let c = default_cfg();
        assert!(c.binds.len() > 40);
        assert_eq!(c.layout, Layout::Tile);
        let sup = Mods { logo: true, ..Mods::default() };
        assert_eq!(c.find(sup, keysym("Return").unwrap()), Some(&Action::Spawn("foot".into())));
        assert_eq!(c.find(sup, keysym("3").unwrap()), Some(&Action::View(WsSel::N(3))));
    }

    #[test]
    fn shift_variant_is_a_different_binding() {
        let c = default_cfg();
        let ss = Mods { logo: true, shift: true, ..Mods::default() };
        assert_eq!(c.find(ss, keysym("q").unwrap()), Some(&Action::Quit));
        assert_eq!(c.find(ss, keysym("Q").unwrap()), Some(&Action::Quit));
    }

    #[test]
    fn user_bind_replaces_and_unbind_removes() {
        let mut c = default_cfg();
        let sup = Mods { logo: true, ..Mods::default() };
        c.apply("bind = mod, t, layout grid\nunbind = mod, f\n");
        assert_eq!(c.find(sup, keysym("t").unwrap()), Some(&Action::Layout(LayoutSel::Set(Layout::Grid))));
        assert_eq!(c.find(sup, keysym("f").unwrap()), None);
    }

    #[test]
    fn mod_can_be_alt() {
        let mut c = default_cfg();
        c.apply("mod = alt\nbind = mod, p, spawn foot\n");
        let alt = Mods { alt: true, ..Mods::default() };
        assert_eq!(c.find(alt, keysym("p").unwrap()), Some(&Action::Spawn("foot".into())));
    }

    #[test]
    fn errors_name_the_line() {
        let mut c = Config::default();
        let e = c.apply("gap = 3\nbogus = 1\nbind = mod, nokey, close\nlayout = wat\n");
        assert_eq!(e.len(), 3);
        assert!(e[0].starts_with("line 2"));
        assert_eq!(c.gap, 3);
    }

    #[test]
    fn actions_parse() {
        assert_eq!(parse_action("mfact -0.05"), Ok(Action::Mfact(-0.05)));
        assert_eq!(parse_action("view next"), Ok(Action::View(WsSel::Dir(Dir::Next))));
        assert_eq!(parse_action("monitor send prev"), Ok(Action::SendMon(Dir::Prev)));
        assert_eq!(parse_action("layout tab"), Ok(Action::Layout(LayoutSel::Set(Layout::Tab))));
        assert!(parse_action("view 0").is_err());
        assert!(parse_action("layout nope").is_err());
        assert!(parse_action("").is_err());
    }
}
