//! Ustawienia > Skroty and Autostart. Shortcuts: every bind, filtered by typing; Enter on one removes it (its line in your
//! config, and an `unbind` when the default config has it); typing `mod+x, spawn foot` offers "Dodaj skrot" on top. Autostart:
//! the `exec =` lines and the files of the autostart folder, a file being on while it is executable (Space flips that).

use std::os::unix::fs::PermissionsExt;

use super::menu::{e, head, Act, Entry};
use crate::config::{keysym, Config, Mods};
use crate::Desktop;

/// `text` without the `bind =` lines for which `same("MODS, key")` is true.
pub fn strip_binds(text: &str, same: impl Fn(&str) -> bool) -> String {
    let mut out: Vec<&str> = Vec::new();
    for l in text.lines() {
        let drop = !l.trim_start().starts_with('#')
            && l.split_once('=').is_some_and(|(k, v)| {
                let mut p = v.splitn(3, ',');
                k.trim() == "bind" && matches!((p.next(), p.next()), (Some(m), Some(key)) if same(&format!("{}, {}", m.trim(), key.trim())))
            });
        if !drop {
            out.push(l);
        }
    }
    if out.is_empty() { String::new() } else { out.join("\n") + "\n" }
}

/// "Super+Shift+Return" -> "super+shift, Return" (the text of an `unbind` line after the `=`).
pub fn unbind_text(label: &str) -> String {
    let mut parts: Vec<&str> = label.split('+').collect();
    let key = parts.pop().unwrap_or("");
    format!("{}, {key}", parts.iter().map(|m| m.to_lowercase()).collect::<Vec<_>>().join("+"))
}

fn key_of(c: &Config, text: &str) -> Option<(Mods, u32)> {
    let (m, k) = text.split_once(',')?;
    Some((c.parse_mods(m.trim()).ok()?, keysym(k.trim())?))
}

/// Rewrite the user config through `f` (temp file + rename).
fn edit_user_config(f: impl FnOnce(&str) -> String) -> std::io::Result<()> {
    let path = crate::config::user_path().ok_or_else(|| std::io::Error::other("no config directory"))?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, f(&old))?;
    std::fs::rename(tmp, path)
}

fn with_line(text: &str, line: &str) -> String {
    let mut t = text.to_string();
    if !t.is_empty() && !t.ends_with('\n') {
        t.push('\n');
    }
    t + line + "\n"
}

/// Every file of the autostart folder and whether it is on (executable).
pub fn autostart_all() -> Vec<(String, bool)> {
    let Some(dir) = crate::config::config_dir().map(|d| d.join("autostart")) else { return Vec::new() };
    let mut v: Vec<(String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|f| f.path().is_file() && !f.file_name().to_string_lossy().starts_with('.'))
        .map(|f| (f.file_name().to_string_lossy().into_owned(), std::fs::metadata(f.path()).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)))
        .collect();
    v.sort();
    v
}

impl Desktop {
    pub fn shortcuts_entries(&self, query: &str) -> Vec<Entry> {
        let mut v = Vec::new();
        if query.contains(',') {
            v.push(e(&format!("Dodaj skrot: {}", query.trim()), Act::AddBind(query.trim().to_string())));
        } else {
            v.push(head("wpisz `mod+x, akcja` zeby dodac skrot, albo slowo zeby szukac; Enter na skrocie usuwa"));
        }
        v.push(head("skroty"));
        let mut b: Vec<&crate::config::Binding> = self.config.binds.iter().collect();
        b.sort_by(|a, b| a.label.cmp(&b.label));
        v.extend(b.into_iter().map(|b| e(&format!("{:<22} {}", b.label, b.act), Act::DelBind(b.label.clone()))));
        v
    }

    pub fn autostart_entries(&self) -> Vec<Entry> {
        let mut v = vec![head("pliki w folderze autostart: wlaczony = wykonywalny, spacja przelacza")];
        let files = autostart_all();
        if files.is_empty() {
            v.push(e("(pusty folder; wrzuc tam skrypt)", Act::Nop));
        }
        v.extend(files.into_iter().map(|(n, on)| e(&n, Act::AutoToggle { name: n.clone(), on })));
        if !self.config.exec.is_empty() {
            v.push(head("linie `exec =` z configu (edytuj config)"));
            v.extend(self.config.exec.iter().map(|c| e(c, Act::Nop)));
        }
        v.push(head("teraz"));
        v.push(e("Uruchom wszystko jeszcze raz", Act::Ctl("autostart run".into())));
        v
    }

    /// `autostart:NAME` from the menu: flip the executable bit.
    pub fn autostart_toggle(&mut self, name: &str, on: bool) -> String {
        if name.contains('/') || name.starts_with('.') {
            return "error: not a file of the autostart folder".into();
        }
        let Some(path) = crate::config::config_dir().map(|d| d.join("autostart").join(name)) else { return "error: no config directory".into() };
        let r = std::fs::metadata(&path).and_then(|m| {
            let mode = m.permissions().mode();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(if on { mode | 0o111 } else { mode & !0o111 }))
        });
        match r {
            Ok(()) => "ok".into(),
            Err(err) => format!("error: {name}: {err}"),
        }
    }

    /// `bind_add("mod+x, spawn foot")`: checked, then written into the user config.
    pub fn bind_add(&mut self, text: &str) -> String {
        let line = format!("bind = {}", text.trim());
        let errs = self.config.clone().apply(&line);
        if let Some(err) = errs.first() {
            return format!("error: {err}");
        }
        let c = self.config.clone();
        let new = key_of(&c, text.trim().splitn(3, ',').take(2).collect::<Vec<_>>().join(",").as_str());
        let r = edit_user_config(|t| with_line(&strip_binds(t, |k| key_of(&c, k) == new), &line));
        match r {
            Ok(()) => {
                self.load_config();
                "ok".into()
            }
            Err(err) => format!("error: the config was not written: {err}"),
        }
    }

    /// Remove the shortcut shown as `label`.
    pub fn bind_del(&mut self, label: &str) -> String {
        let text = unbind_text(label);
        let c = self.config.clone();
        let target = key_of(&c, &text);
        if target.is_none() {
            return format!("error: cannot read {label:?}");
        }
        let mut defaults = Config::default();
        defaults.apply(crate::config::DEFAULT);
        let in_default = defaults.binds.iter().any(|b| Some((b.mods, b.sym)) == target);
        let r = edit_user_config(|t| {
            let t = strip_binds(t, |k| key_of(&c, k) == target);
            if in_default { with_line(&t, &format!("unbind = {text}")) } else { t }
        });
        match r {
            Ok(()) => {
                self.load_config();
                "ok".into()
            }
            Err(err) => format!("error: the config was not written: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_matching_bind_lines_go() {
        let t = "gap = 3\nbind = mod, x, spawn a\n# bind = mod, x, spawn b\nbind = mod, y, spawn c\n";
        assert_eq!(strip_binds(t, |k| k == "mod, x"), "gap = 3\n# bind = mod, x, spawn b\nbind = mod, y, spawn c\n");
        assert_eq!(strip_binds("bind = mod, x, a\n", |_| true), "");
    }

    #[test]
    fn a_label_becomes_an_unbind_text() {
        assert_eq!(unbind_text("Super+Shift+Return"), "super+shift, Return");
        assert_eq!(unbind_text("Print"), ", Print");
    }

    #[test]
    fn a_line_is_appended_on_its_own_row() {
        assert_eq!(with_line("a = 1", "b = 2"), "a = 1\nb = 2\n");
        assert_eq!(with_line("", "b = 2"), "b = 2\n");
    }

    #[test]
    fn the_same_key_written_two_ways_is_the_same_key() {
        let c = Config::default();
        assert_eq!(key_of(&c, "super+shift, Return"), key_of(&c, "mod+shift, Return"));
        assert_ne!(key_of(&c, "mod, x"), key_of(&c, "mod, y"));
    }
}
