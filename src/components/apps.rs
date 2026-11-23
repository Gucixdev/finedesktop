//! Desktop entries (`*.desktop`) for the menu: name, command, terminal flag, and where it comes from
//! (Flatpak exports or native). Plain text parsing, no libraries.

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct App {
    /// desktop file name, e.g. `org.mozilla.firefox.desktop`
    pub id: String,
    pub name: String,
    pub exec: String,
    pub terminal: bool,
    pub flatpak: bool,
    pub game: bool,
    /// the `Icon=` value: a name or an absolute path, empty when the file has none
    pub icon: String,
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

fn parse(text: &str, flatpak: bool, id: &str) -> Option<App> {
    let (mut name, mut exec, mut terminal, mut hidden, mut cats, mut app, mut icon) = (None, None, false, false, String::new(), true, String::new());
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
            "Icon" if icon.is_empty() => icon = v.trim().to_string(),
            "Categories" => cats = v.to_string(),
            "Type" => app = v == "Application",
            _ => {}
        }
    }
    if hidden || !app {
        return None;
    }
    Some(App { id: id.to_string(), name: name?, exec: exec.filter(|e| !e.is_empty())?, terminal, flatpak, game: cats.split(';').any(|c| c == "Game"), icon })
}

/// The icon of the application a notification came from: its name (`firefox`, `Discord`) against the app's name, then the
/// last part of its desktop id (`org.mozilla.firefox`); the icon name or path as the desktop file gives it.
pub fn icon_for(app: &str) -> Option<String> {
    let want = app.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    let all = scan();
    all.iter()
        .find(|a| a.name.to_lowercase() == want)
        .or_else(|| all.iter().find(|a| a.id.trim_end_matches(".desktop").rsplit('.').next().is_some_and(|l| l.to_lowercase() == want)))
        .map(|a| a.icon.clone())
        .filter(|i| !i.is_empty())
}

/// Every application once (the first directory wins), sorted by name.
pub fn scan() -> Vec<App> {
    let mut v: Vec<App> = Vec::new();
    for d in dirs() {
        let flatpak = d.to_string_lossy().contains("/flatpak/exports/");
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if e.path().extension().is_some_and(|x| x == "desktop") {
                if let Some(a) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse(&t, flatpak, &e.file_name().to_string_lossy())) {
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
        let a = parse("[Desktop Entry]\nType=Application\nName=Foo\nExec=foo --x %U %%\nCategories=Game;\n[Desktop Action x]\nName=Other\n", true, "foo.desktop").unwrap();
        assert_eq!((a.name.as_str(), a.exec.as_str(), a.game, a.flatpak), ("Foo", "foo --x %", true, true));
    }

    #[test]
    fn hidden_and_nameless_entries_are_skipped() {
        assert_eq!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nIcon=org.a.A\n[Desktop Action x]\nIcon=other\n", false, "a.desktop").unwrap().icon, "org.a.A");
        assert!(parse("[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n", false, "a.desktop").is_none());
        assert!(parse("[Desktop Entry]\nType=Application\nExec=a\n", false, "a.desktop").is_none());
    }
}
