//! `ctl set [-s] KEY VALUE` and `ctl get KEY`: every config key (`theme`, `font`, `font_scale`, `gap`, ...) read and
//! changed in the running desktop, with the same names and values as the config file. `set` applies at once (hot
//! reload of what depends on it); without `-s` it lasts until the next `ctl reload`, with `-s` it is also written to
//! the user config. `get` answers from what is in force: theme, font, font_scale and gap from the live config, any
//! other key set at runtime, else from the user config, else from the built-in one.

use crate::config::{Config, DEFAULT};
use crate::state::Desktop;

/// `KEY VALUE` or `KEY = VALUE` -> the pair.
fn split(arg: &str) -> Option<(&str, &str)> {
    let arg = arg.trim();
    let (k, v) = arg.split_once(|c: char| c == '=' || c.is_whitespace())?;
    let v = v.trim().trim_start_matches('=').trim();
    (!k.is_empty() && !v.is_empty()).then_some((k, v))
}

/// The last `key = value` line of `text` (inline comments need " # ").
fn last_value(text: &str, key: &str) -> Option<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.split(" # ").next().unwrap_or(l).trim();
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
        .last()
}

impl Desktop {
    pub fn set_cmd(&mut self, arg: &str) -> String {
        let arg = arg.trim();
        let (save, arg) = match arg.strip_prefix("-s") {
            Some(r) if r.starts_with(char::is_whitespace) => (true, r.trim()),
            _ => (false, arg),
        };
        let Some((key, val)) = split(arg) else {
            return "error: usage: set [-s] KEY VALUE".into();
        };
        let mut c: Config = self.config.clone();
        if let Some(e) = c.apply(&format!("{key} = {val}")).into_iter().next() {
            return format!("error: {e}");
        }
        self.config = c;
        if key == "font" {
            if let Err(e) = crate::components::font::Font::select(&self.config.font) {
                return format!("error: {e}");
            }
        }
        self.runtime_set.retain(|(k, _)| k != key);
        if save {
            if let Err(e) = crate::components::settings::settings_preferaps::persist(key, val) {
                return format!("error: {e}");
            }
        } else {
            self.runtime_set.push((key.to_string(), val.to_string()));
        }
        self.config_applied();
        self.retile();
        if key == "theme" {
            self.emit(crate::components::hooks::Event::theme(val));
        }
        "ok".into()
    }

    pub fn get_cmd(&mut self, key: &str) -> String {
        let key = key.trim();
        let c = &self.config;
        match key {
            "theme" => return c.theme.name.clone(),
            "font" => return c.font.clone(),
            "font_scale" => return c.font_scale.to_string(),
            "gap" => return c.gap.to_string(),
            _ => {}
        }
        if let Some((_, v)) = self.runtime_set.iter().rev().find(|(k, _)| k == key) {
            return v.clone();
        }
        let user = crate::config::user_path().and_then(|p| std::fs::read_to_string(p).ok());
        user.and_then(|t| last_value(&t, key))
            .or_else(|| last_value(DEFAULT, key))
            .unwrap_or_else(|| format!("error: no value for {key:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_takes_both_spellings() {
        assert_eq!(split("gap 8"), Some(("gap", "8")));
        assert_eq!(split("gap = 8"), Some(("gap", "8")));
        assert_eq!(split("gap=8"), Some(("gap", "8")));
        assert_eq!(split("gap"), None);
    }

    #[test]
    fn last_value_wins_and_comments_go() {
        assert_eq!(last_value("gap = 4\n# x\ngap = 6 # six\n", "gap").as_deref(), Some("6"));
        assert_eq!(last_value("gap = 4\n", "font"), None);
    }
}
