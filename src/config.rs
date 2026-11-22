//! Config: one tiny line format shared by the built-in defaults, the user file and the IPC
//! socket (actions). No dependencies, every error is reported with its line number.

use smithay::input::keyboard::xkb;

use crate::components::engine::layout_engine::Layout;

pub const DEFAULT: &str = include_str!("default.conf");

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
    Focus(Dir),
    Zoom,
    Layout(LayoutSel),
    Mfact(f32),
    Nmaster(i32),
    View(WsSel),
    Tag(WsSel),
    FocusMon(Dir),
    SendMon(Dir),
    State,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub mods: Mods,
    pub sym: u32,
    pub action: Action,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub modkey: Mods,
    pub terminal: String,
    pub menu: String,
    pub gap: i32,
    pub mfact: f32,
    pub nmaster: usize,
    pub bar_h: i32,
    pub layout: Layout,
    pub workspaces: usize,
    pub background: [f32; 4],
    pub binds: Vec<Binding>,
    pub exec: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            modkey: Mods { logo: true, ..Mods::default() },
            terminal: "foot".into(),
            menu: "dmenu_run".into(),
            gap: 0,
            mfact: 0.55,
            nmaster: 1,
            bar_h: 22,
            layout: Layout::Tile,
            workspaces: 9,
            background: [0.0, 0.0, 0.0, 1.0],
            binds: Vec::new(),
            exec: Vec::new(),
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
        "state" => Action::State,
        "zoom" => Action::Zoom,
        "focus" => Action::Focus(parse_dir(rest)?),
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
            "menu" => self.menu = expand_home(val),
            "gap" => self.gap = n(val)?.max(0),
            "mfact" => self.mfact = f(val)?.clamp(0.1, 0.9),
            "nmaster" => self.nmaster = n(val)?.max(0) as usize,
            "bar_h" => self.bar_h = n(val)?.max(0),
            "workspaces" => self.workspaces = (n(val)?.clamp(1, 32)) as usize,
            "layout" => self.layout = Layout::from_name(val).ok_or_else(|| format!("unknown layout {val:?}"))?,
            "background" => self.background = parse_color(val)?,
            "exec" => self.exec.push(val.into()),
            "bind" => {
                let mut p = val.splitn(3, ',').map(str::trim);
                let (mods, k, act) = (p.next().unwrap_or(""), p.next().ok_or("bind = MODS, key, action")?, p.next().ok_or("bind = MODS, key, action")?);
                let mods = self.parse_mods(mods)?;
                let sym = keysym(k).ok_or_else(|| format!("unknown key {k:?}"))?;
                let act = act.replace("$terminal", &self.terminal.clone()).replace("$menu", &self.menu.clone());
                let action = parse_action(&act)?;
                self.binds.retain(|b| !(b.mods == mods && b.sym == sym));
                self.binds.push(Binding { mods, sym, action });
            }
            "unbind" => {
                let (mods, k) = val.split_once(',').ok_or("unbind = MODS, key")?;
                let (mods, sym) = (self.parse_mods(mods)?, keysym(k.trim()).ok_or("unknown key")?);
                self.binds.retain(|b| !(b.mods == mods && b.sym == sym));
            }
            other => return Err(format!("unknown setting {other:?}")),
        }
        Ok(())
    }

    /// Defaults, then the user file. Errors are logged, never fatal.
    pub fn load() -> Config {
        let mut c = Config::default();
        for e in c.apply(DEFAULT) {
            tracing::error!("built-in config: {e}");
        }
        if let Some(path) = user_path() {
            if let Ok(text) = std::fs::read_to_string(&path) {
                for e in c.apply(&text) {
                    tracing::warn!("{}: {e}", path.display());
                }
            }
        }
        c
    }

    pub fn find(&self, mods: Mods, sym: u32) -> Option<&Action> {
        let sym = norm(sym);
        self.binds.iter().find(|b| b.mods == mods && b.sym == sym).map(|b| &b.action)
    }
}

pub fn user_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))?;
    Some(base.join("finedesktop").join("config"))
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
    use super::*;

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
