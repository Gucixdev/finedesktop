//! Small helpers around key handling and process spawning. The bindings themselves live in `config.rs`.

use smithay::input::keyboard::ModifiersState;

use crate::config::Mods;

pub fn mods_of(m: &ModifiersState) -> Mods {
    Mods { logo: m.logo, alt: m.alt, ctrl: m.ctrl, shift: m.shift }
}

/// Run `cmd` through `sh -c`, detached. Children inherit WAYLAND_DISPLAY and FD_SOCK.
pub fn spawn(cmd: &str) {
    if let Err(e) = std::process::Command::new("sh").arg("-c").arg(cmd).spawn() {
        tracing::warn!("spawn {cmd:?}: {e}");
    }
}
