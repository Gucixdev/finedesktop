//! Key bindings, dwl-style. The modifier is Super by default; `FD_MOD=alt` swaps it,
//! which is what you want when running nested inside another compositor.

use smithay::input::keyboard::{ModifiersState, keysyms as k};

use crate::components::engine::layout_engine::Layout;

#[derive(Clone, Copy, Debug)]
pub enum Mod {
    Super,
    Alt,
}

impl Mod {
    pub fn from_env() -> Mod {
        match std::env::var("FD_MOD").as_deref() {
            Ok("alt") => Mod::Alt,
            _ => Mod::Super,
        }
    }
    fn held(self, m: &ModifiersState) -> bool {
        match self {
            Mod::Super => m.logo,
            Mod::Alt => m.alt,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Spawn(String),
    Close,
    Quit,
    FocusNext,
    FocusPrev,
    Zoom,
    SetLayout(Layout),
    CycleLayout(bool),
    Mfact(f32),
    Nmaster(i32),
}

pub fn terminal() -> String {
    std::env::var("TERMINAL").unwrap_or_else(|_| "foot".into())
}

pub fn menu() -> String {
    std::env::var("FD_MENU").unwrap_or_else(|_| {
        format!("{}/.config/src/menu", std::env::var("HOME").unwrap_or_default())
    })
}

pub fn spawn(cmd: &str) {
    if let Err(e) = std::process::Command::new("sh").arg("-c").arg(cmd).spawn() {
        tracing::warn!("spawn {cmd:?}: {e}");
    }
}

/// Map a pressed key to an action. `sym` is the raw keysym (shift state ignored), so
/// Shift+letter is matched through `m.shift` and not through the uppercase symbol.
pub fn lookup(modkey: Mod, m: &ModifiersState, sym: u32) -> Option<Action> {
    if !modkey.held(m) || m.ctrl {
        return None;
    }
    let sym = if (k::KEY_A..=k::KEY_Z).contains(&sym) { sym + 0x20 } else { sym };
    Some(match (m.shift, sym) {
        (false, k::KEY_Return) => Action::Spawn(terminal()),
        (false, k::KEY_w) => Action::Spawn(menu()),
        (false, k::KEY_q) | (true, k::KEY_c) => Action::Close,
        (true, k::KEY_q) => Action::Quit,
        (false, k::KEY_j) => Action::FocusNext,
        (false, k::KEY_k) => Action::FocusPrev,
        (true, k::KEY_Return) => Action::Zoom,
        (false, k::KEY_space) => Action::CycleLayout(true),
        (true, k::KEY_space) => Action::CycleLayout(false),
        (false, k::KEY_t) => Action::SetLayout(Layout::Tile),
        (false, k::KEY_f) => Action::SetLayout(Layout::Float),
        (false, k::KEY_m) => Action::SetLayout(Layout::Monocle),
        (false, k::KEY_d) => Action::SetLayout(Layout::Deck),
        (false, k::KEY_g) => Action::SetLayout(Layout::Grid),
        (false, k::KEY_s) => Action::SetLayout(Layout::Stack),
        (false, k::KEY_y) => Action::SetLayout(Layout::Tab),
        (false, k::KEY_h) => Action::Mfact(-0.05),
        (false, k::KEY_l) => Action::Mfact(0.05),
        (false, k::KEY_i) => Action::Nmaster(1),
        (true, k::KEY_i) => Action::Nmaster(-1),
        _ => return None,
    })
}
