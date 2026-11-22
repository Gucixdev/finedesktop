//! Desktop entries (`*.desktop`) for the menu: name, command, terminal flag, and where it comes from
//! (Flatpak exports or native). Plain text parsing, no libraries.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    pub exec: String,
    pub terminal: bool,
    pub flatpak: bool,
    pub game: bool,
}

fn dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let home = env("HOME").unwrap_or_else(|| "/".into());
    let mut v = vec![PathBuf::from(env("XDG_DATA_HOME").unwrap_or(format!("{home}/.local/share"))).join("applications")];
    for d in env("XDG_DATA_DIRS").unwrap_or("/usr/local/share:/usr/share".into()).split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d).join("applications"));
    }
    if let Some(f) = env("FLATPAK_USER_DIR") {
        v.push(PathBuf::from(f).join("exports/share/applications"));
    }
    v.push(PathBuf::from(format!("{home}/.local/share/flatpak/exports/share/applications")));
    v.push("/var/lib/flatpak/exports/share/applications".into());
    v
}

/// Drop the `%f %U ...` field codes of an Exec line; `%%` is a literal percent.
fn strip_codes(exec: &str) -> String {
    let mut out = String::new();
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
        } else if it.peek() == Some(&'%') {
            it.next();
            out.push('%');
        } else {
            it.next();
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse(text: &str, flatpak: bool) -> Option<App> {
    let (mut name, mut exec, mut terminal, mut hidden, mut cats, mut app) = (None, None, false, false, String::new(), true);
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if !inside {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "Name" => name = name.or_else(|| Some(v.to_string())),
            "Exec" => exec = exec.or_else(|| Some(strip_codes(v))),
            "Terminal" => terminal = v == "true",
            "NoDisplay" | "Hidden" => hidden |= v == "true",
            "Categories" => cats = v.to_string(),
            "Type" => app = v == "Application",
            _ => {}
        }
    }
    if hidden || !app {
        return None;
    }
    Some(App { name: name?, exec: exec.filter(|e| !e.is_empty())?, terminal, flatpak, game: cats.split(';').any(|c| c == "Game") })
}

/// Every application once (the first directory wins), sorted by name.
pub fn scan() -> Vec<App> {
    let mut v: Vec<App> = Vec::new();
    for d in dirs() {
        let flatpak = d.to_string_lossy().contains("/flatpak/exports/");
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x == "desktop") {
                if let Some(a) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse(&t, flatpak)) {
                    v.push(a);
                }
            }
        }
    }
    v.sort_by_key(|a| a.name.to_lowercase());
    v.dedup_by(|a, b| a.name == b.name);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_entry_and_strips_field_codes() {
        let a = parse("[Desktop Entry]\nType=Application\nName=Foo\nExec=foo --x %U %%\nCategories=Game;\n[Desktop Action x]\nName=Other\n", true).unwrap();
        assert_eq!((a.name.as_str(), a.exec.as_str(), a.game, a.flatpak), ("Foo", "foo --x %", true, true));
    }

    #[test]
    fn hidden_and_nameless_entries_are_skipped() {
        assert!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n", false).is_none());
        assert!(parse("[Desktop Entry]\nType=Application\nExec=a\n", false).is_none());
    }
}
