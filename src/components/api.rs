//! The wallpaper API: one default picture, optionally one per output, a fit mode; shown by `swaybg`.
//! `ctl wallpaper ...` (parsed by `ast.rs`) reads and changes it, the settings panel and the config use the same keys:
//! `wallpaper = PATH`, `wallpaper_mode = fill|fit|stretch|center|tile`, `wallpaper_OUTPUT = PATH`.

use super::ast::{self, Cmd, Mode};
use crate::state::Desktop;
use std::path::{Path, PathBuf};

/// What the wallpaper is, as the config holds it.
pub struct Spec<'a> {
    pub default: &'a str,
    pub per_output: &'a [(String, String)],
    pub mode: Mode,
}

/// swaybg's argument list: the default picture on every output (`-o '*'`), then one `-o NAME` group per own picture.
pub fn swaybg_args(s: &Spec) -> Vec<String> {
    let mut a = Vec::new();
    let mut group = |out: &str, pic: &str| {
        a.extend(["-o".to_string(), out.to_string(), "-i".to_string(), pic.to_string(), "-m".to_string(), s.mode.name().to_string()]);
    };
    if !s.default.is_empty() {
        group("*", s.default);
    }
    for (out, pic) in s.per_output {
        group(out, pic);
    }
    a
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The shell line that replaces the running swaybg (and only stops it when there is no picture).
pub fn shell_line(args: &[String]) -> String {
    if args.is_empty() {
        return "pkill -x swaybg".into();
    }
    format!("pkill -x swaybg; exec swaybg {}", args.iter().map(|x| quote(x)).collect::<Vec<_>>().join(" "))
}

/// The picture `step` places after `cur` in `list` (wrapping); the first one when `cur` is not in the list.
pub fn cycle(list: &[PathBuf], cur: &str, step: i32) -> Option<String> {
    if list.is_empty() {
        return None;
    }
    let n = list.len() as i32;
    let i = list.iter().position(|p| p.to_string_lossy() == cur).map_or(0, |i| (i as i32 + step).rem_euclid(n) as usize);
    Some(list[i].to_string_lossy().into_owned())
}

fn absolute(p: &str) -> Result<String, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let p = match p.strip_prefix("~/") {
        Some(r) => format!("{home}/{r}"),
        None => p.to_string(),
    };
    if !p.starts_with('/') {
        return Err(format!("{p:?} is relative; the compositor does not know your directory, give an absolute path or ~/..."));
    }
    if !Path::new(&p).is_file() {
        return Err(format!("{p:?} is not a file"));
    }
    Ok(p)
}

impl Desktop {
    /// `ctl wallpaper ...`: see `ast::USAGE`. Changes are written to the config and shown at once.
    pub fn wallpaper_cmd(&mut self, args: &str) -> String {
        match ast::parse(args) {
            Ok(cmd) => self.wallpaper_run(cmd).unwrap_or_else(|e| format!("error: {e}")),
            Err(e) => format!("error: {e}"),
        }
    }

    fn wallpaper_run(&mut self, cmd: Cmd) -> Result<String, String> {
        let set = |d: &mut Desktop, key: &str, val: &str| -> Result<String, String> {
            crate::components::settings::settings_preferaps::persist(key, val).map_err(|e| format!("the config was not written: {e}"))?;
            d.load_config();
            Ok("ok".into())
        };
        match cmd {
            Cmd::Show => {
                let c = &self.config;
                let mut v = vec![format!("* {} ({})", if c.wallpaper.is_empty() { "none" } else { &c.wallpaper }, c.wallpaper_mode.name())];
                v.extend(c.wallpaper_outputs.iter().map(|(o, p)| format!("{o} {p}")));
                Ok(v.join("\n"))
            }
            Cmd::List => {
                let cur = self.config.wallpaper.clone();
                let list = super::settings::settings_hardware::wallpapers();
                Ok(list.iter().map(|p| format!("{} {}", if p.to_string_lossy() == cur { '*' } else { ' ' }, p.display())).collect::<Vec<_>>().join("\n"))
            }
            Cmd::Next | Cmd::Prev => {
                let list = super::settings::settings_hardware::wallpapers();
                let step = if cmd == Cmd::Next { 1 } else { -1 };
                let pic = cycle(&list, &self.config.wallpaper, step).ok_or("no pictures found (~/Pictures, ~/Pictures/Wallpapers, ~/.local/share/backgrounds)")?;
                set(self, "wallpaper", &pic)
            }
            Cmd::Mode(m) => set(self, "wallpaper_mode", m.name()),
            Cmd::Set { output, path } => {
                let path = absolute(&path)?;
                match output {
                    None => set(self, "wallpaper", &path),
                    Some(o) => {
                        self.known_output(&o)?;
                        set(self, &format!("wallpaper_{o}"), &path)
                    }
                }
            }
            Cmd::Clear { output: None } => set(self, "wallpaper", ""),
            Cmd::Clear { output: Some(o) } => set(self, &format!("wallpaper_{o}"), ""),
        }
    }

    fn known_output(&self, name: &str) -> Result<(), String> {
        if self.mons.iter().any(|m| m.output.name() == name) {
            Ok(())
        } else {
            Err(format!("no output {name:?} (have: {})", self.mons.iter().map(|m| m.output.name()).collect::<Vec<_>>().join(" ")))
        }
    }

    /// Start swaybg for the configured pictures (replacing the old one), only when that line changed.
    pub fn wallpaper_apply(&mut self) {
        let c = &self.config;
        let usable = |p: &str| {
            let ok = Path::new(p).is_file();
            if !ok {
                tracing::warn!("wallpaper {p:?} is not a file");
            }
            ok
        };
        let default = if usable_or_empty(&c.wallpaper, &usable) { c.wallpaper.clone() } else { String::new() };
        let own: Vec<(String, String)> = c.wallpaper_outputs.iter().filter(|(_, p)| usable(p)).cloned().collect();
        let line = shell_line(&swaybg_args(&Spec { default: &default, per_output: &own, mode: c.wallpaper_mode }));
        if self.wallpaper_shown.as_deref() == Some(line.as_str()) {
            return;
        }
        self.wallpaper_shown = Some(line.clone());
        crate::keys::spawn(&line);
    }
}

fn usable_or_empty(p: &str, usable: &impl Fn(&str) -> bool) -> bool {
    !p.is_empty() && usable(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec<'a>(default: &'a str, own: &'a [(String, String)]) -> Spec<'a> {
        Spec { default, per_output: own, mode: Mode::Fill }
    }

    #[test]
    fn no_picture_stops_swaybg_and_one_picture_covers_every_output() {
        assert_eq!(shell_line(&swaybg_args(&spec("", &[]))), "pkill -x swaybg");
        assert_eq!(swaybg_args(&spec("/a.png", &[])), ["-o", "*", "-i", "/a.png", "-m", "fill"]);
    }

    #[test]
    fn an_output_with_its_own_picture_follows_the_default() {
        let own = [("DP-1".to_string(), "/b.png".to_string())];
        let a = swaybg_args(&spec("/a.png", &own));
        assert_eq!(a[6..], ["-o", "DP-1", "-i", "/b.png", "-m", "fill"]);
        assert_eq!(swaybg_args(&spec("", &own)).len(), 6);
    }

    #[test]
    fn quotes_in_a_path_survive_the_shell_line() {
        let l = shell_line(&swaybg_args(&spec("/it's.png", &[])));
        assert!(l.contains("'/it'\\''s.png'"), "{l}");
    }

    #[test]
    fn next_and_prev_wrap_and_an_unknown_picture_starts_at_the_first() {
        let l: Vec<PathBuf> = ["/a", "/b", "/c"].iter().map(PathBuf::from).collect();
        assert_eq!(cycle(&l, "/c", 1).as_deref(), Some("/a"));
        assert_eq!(cycle(&l, "/a", -1).as_deref(), Some("/c"));
        assert_eq!(cycle(&l, "", 1).as_deref(), Some("/a"));
        assert_eq!(cycle(&[], "", 1), None);
    }

    #[test]
    fn a_relative_path_is_refused() {
        assert!(absolute("pic.png").unwrap_err().contains("relative"));
        assert!(absolute("/definitely/not/here.png").unwrap_err().contains("not a file"));
    }
}
