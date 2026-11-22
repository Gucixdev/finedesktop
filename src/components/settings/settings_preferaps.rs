//! Preferred applications: which program is "the" terminal, browser, editor and file manager.
//! Each role is one config line `prefer_<role> = COMMAND`, usable in binds as `$terminal $browser $editor $files`,
//! readable and settable with `ctl prefer [ROLE [COMMAND]]`, and pickable from the menu (Ustawienia > Preferowane
//! aplikacje). Setting a role from the menu or ctl also writes the line into the user config, nothing else is touched.

use std::collections::BTreeMap;

use crate::{Desktop, components::apps::App};

/// The roles, in the order the menu shows them.
pub const ROLES: [&str; 4] = ["terminal", "browser", "editor", "files"];

pub fn defaults() -> BTreeMap<String, String> {
    [("browser", "firefox"), ("editor", "foot -e frost"), ("files", "foot -e nnn")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Command line that starts `app`; applications that need a terminal are wrapped in the preferred one.
pub fn command_for(app: &App, terminal: &str) -> String {
    if app.terminal { format!("{terminal} -e {}", app.exec) } else { app.exec.clone() }
}

/// `text` with the line `key = val` replaced (first match) or appended. Every other line stays byte for byte.
pub fn set_line(text: &str, key: &str, val: &str) -> String {
    let new = format!("{key} = {val}");
    let mut done = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            let is_key = l.split_once('=').is_some_and(|(k, _)| k.trim() == key) && !l.trim_start().starts_with('#');
            if is_key && !done {
                done = true;
                new.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !done {
        out.push(new);
    }
    out.join("\n") + "\n"
}

/// Write `key = val` into the user config (temp file + rename, so a crash never leaves half a config).
pub fn persist(key: &str, val: &str) -> std::io::Result<()> {
    let path = crate::config::user_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, set_line(&old, key, val))?;
    std::fs::rename(tmp, path)
}

impl Desktop {
    /// `ctl prefer` lists the roles, `ctl prefer ROLE` prints one, `ctl prefer ROLE COMMAND` sets and saves it.
    pub fn prefer(&mut self, args: &str) -> String {
        let (role, cmd) = args.trim().split_once(char::is_whitespace).map_or((args.trim(), ""), |(a, b)| (a, b.trim()));
        if role.is_empty() {
            return ROLES.iter().map(|r| format!("{r} = {}", self.config.prefer_get(r).unwrap_or_default())).collect::<Vec<_>>().join("\n");
        }
        if cmd.is_empty() {
            return match self.config.prefer_get(role) {
                Some(c) => c,
                None => format!("error: unknown role {role:?} (have: {})", ROLES.join(" ")),
            };
        }
        if let Err(e) = self.config.prefer_set(role, cmd) {
            return format!("error: {e}");
        }
        match persist(&format!("prefer_{role}"), cmd) {
            Ok(()) => "ok".into(),
            Err(e) => format!("error: set for this session, but the config was not written: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_line_replaces_or_appends_and_keeps_the_rest() {
        let t = "# mine\nkb_layout = pl,gb\nprefer_browser = firefox\nexec = x\n";
        assert_eq!(set_line(t, "prefer_browser", "chromium"), "# mine\nkb_layout = pl,gb\nprefer_browser = chromium\nexec = x\n");
        assert_eq!(set_line(t, "prefer_files", "nnn"), format!("{t}prefer_files = nnn\n"));
        assert_eq!(set_line("", "a", "b"), "a = b\n");
    }

    #[test]
    fn a_commented_line_is_not_the_setting() {
        assert_eq!(set_line("# prefer_browser = x\n", "prefer_browser", "y"), "# prefer_browser = x\nprefer_browser = y\n");
    }

    #[test]
    fn terminal_apps_get_the_preferred_terminal() {
        let a = App { name: "htop".into(), exec: "htop".into(), terminal: true, flatpak: false, game: false };
        assert_eq!(command_for(&a, "foot"), "foot -e htop");
        assert_eq!(command_for(&App { terminal: false, ..a }, "foot"), "htop");
    }
}
