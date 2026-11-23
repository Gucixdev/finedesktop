//! Preferred applications: which program is "the" terminal, browser, editor, file manager and shell (bash).
//! Each role is one config line `prefer_<role> = COMMAND`, usable in binds as `$terminal $browser $editor $files`,
//! readable and settable with `ctl prefer [ROLE [COMMAND]]`, and pickable from the menu (Ustawienia > Preferowane
//! aplikacje). Setting a role from the menu or ctl also writes the line into the user config, nothing else is touched.

use std::collections::BTreeMap;

use crate::{Desktop, components::apps::App};

/// The roles, in the order the menu shows them.
pub const ROLES: [&str; 13] = ["terminal", "browser", "editor", "files", "shell", "office", "pdf", "image", "video", "audio", "archive", "cad", "mail"];

/// The first five are programs used by binds (`$terminal` ... `$shell`); the rest say what opens a kind of file.
/// `shell` is the program a terminal window runs (`fd-shell` asks for it); there is no shell of our own, it is bash.
pub const PROGRAMS: usize = 5;

/// The formats a file role covers, for the menu label.
pub fn formats(role: &str) -> &'static str {
    match role {
        "office" => "docx doc odt rtf xlsx xls ods csv pptx ppt odp",
        "pdf" => "pdf epub djvu",
        "image" => "png jpg gif webp svg bmp",
        "video" => "mp4 mkv webm mov avi",
        "audio" => "mp3 flac ogg wav",
        "archive" => "zip tar gz xz 7z rar",
        "cad" => "stl obj 3mf step blend",
        "mail" => "mailto:",
        _ => "",
    }
}

/// Generic defaults that work anywhere: the system's own openers and `$EDITOR`. Your own choices go in your config
/// (`prefer_browser = firefox`) or are made in the menu.
pub fn defaults() -> BTreeMap<String, String> {
    [("shell", "bash"), ("browser", "xdg-open https://"), ("editor", "$terminal -e ${EDITOR:-vi}"), ("files", "xdg-open .")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// MIME types whose system default follows a role (the terminal has no standard one).
pub fn mimes(role: &str) -> &'static [&'static str] {
    match role {
        "browser" => &["x-scheme-handler/http", "x-scheme-handler/https", "text/html", "application/xhtml+xml"],
        "files" => &["inode/directory"],
        "editor" => &["text/plain"],
        "office" => &[
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document", "application/msword", "application/vnd.oasis.opendocument.text", "application/rtf",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "application/vnd.ms-excel", "application/vnd.oasis.opendocument.spreadsheet", "text/csv",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation", "application/vnd.ms-powerpoint", "application/vnd.oasis.opendocument.presentation",
        ],
        "pdf" => &["application/pdf", "application/epub+zip", "image/vnd.djvu"],
        "image" => &["image/png", "image/jpeg", "image/gif", "image/webp", "image/svg+xml", "image/bmp"],
        "video" => &["video/mp4", "video/x-matroska", "video/webm", "video/quicktime", "video/x-msvideo"],
        "audio" => &["audio/mpeg", "audio/flac", "audio/ogg", "audio/x-wav"],
        "archive" => &["application/zip", "application/x-tar", "application/gzip", "application/x-xz", "application/x-7z-compressed", "application/vnd.rar"],
        "cad" => &["model/stl", "model/obj", "model/3mf", "model/step", "application/sla", "application/x-blender"],
        "mail" => &["x-scheme-handler/mailto"],
        _ => &[],
    }
}

/// The desktop file whose command is `cmd` (as `command_for` builds it), if there is one.
pub fn app_id_for<'a>(apps: &'a [App], cmd: &str, terminal: &str) -> Option<&'a str> {
    let bare = cmd.strip_prefix(&format!("{terminal} -e ")).unwrap_or(cmd);
    let first = |s: &str| s.split_whitespace().next().map(|w| w.rsplit('/').next().unwrap_or(w).to_string());
    let want = first(bare)?;
    apps.iter().find(|a| first(&a.exec).as_deref() == Some(want.as_str())).map(|a| a.id.as_str())
}

/// Make `id` the system default for the role's MIME types (`xdg-mime default`). Ok(false): nothing to set.
pub fn set_system_default(role: &str, id: &str) -> Result<bool, String> {
    let types = mimes(role);
    if types.is_empty() {
        return Ok(false);
    }
    let st = std::process::Command::new("xdg-mime").arg("default").arg(id).args(types).status().map_err(|e| format!("xdg-mime: {e}"))?;
    if st.success() { Ok(true) } else { Err(format!("xdg-mime default {id} failed")) }
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
        if let Err(e) = persist(&format!("prefer_{role}"), cmd) {
            return format!("error: set for this session, but the config was not written: {e}");
        }
        // the system default follows when the command is a known desktop application
        let apps = crate::components::apps::scan();
        match app_id_for(&apps, cmd, &self.config.terminal).map(|id| (id.to_string(), set_system_default(role, id))) {
            Some((id, Ok(true))) => format!("ok (system default: {id})"),
            Some((_, Err(e))) => format!("ok, config written; system default not set: {e}"),
            _ => "ok".into(),
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

    fn app(id: &str, exec: &str) -> App {
        App { id: id.into(), name: id.into(), exec: exec.into(), terminal: false, flatpak: false, game: false }
    }

    #[test]
    fn a_command_finds_its_desktop_file() {
        let apps = vec![app("htop.desktop", "htop"), app("org.mozilla.firefox.desktop", "/usr/bin/firefox --new-window")];
        assert_eq!(app_id_for(&apps, "firefox", "foot"), Some("org.mozilla.firefox.desktop"));
        assert_eq!(app_id_for(&apps, "foot -e htop", "foot"), Some("htop.desktop"));
        assert_eq!(app_id_for(&apps, "xdg-open https://", "foot"), None);
    }

    #[test]
    fn roles_map_to_mime_types() {
        assert!(mimes("browser").contains(&"x-scheme-handler/https"));
        assert_eq!(mimes("files"), ["inode/directory"]);
        assert!(mimes("terminal").is_empty());
        for r in &ROLES[PROGRAMS..] {
            assert!(!mimes(r).is_empty() && !formats(r).is_empty(), "{r} covers some formats");
        }
        assert!(mimes("cad").contains(&"model/stl") && formats("office").contains("docx"));
    }

    #[test]
    fn terminal_apps_get_the_preferred_terminal() {
        let a = App { id: "htop.desktop".into(), name: "htop".into(), exec: "htop".into(), terminal: true, flatpak: false, game: false };
        assert_eq!(command_for(&a, "foot"), "foot -e htop");
        assert_eq!(command_for(&App { terminal: false, ..a }, "foot"), "htop");
    }
}
