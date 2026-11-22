//! The freedesktop trash, so nothing is ever removed for good by a script: `ctl trash PATH` moves a file or directory
//! into `$XDG_DATA_HOME/Trash` (with the .trashinfo that file managers read), `ctl trash list` shows what is in there,
//! `ctl trash restore NAME` puts it back. Emptying is left to you. Only the same filesystem: no copy-then-delete.

use std::path::{Path, PathBuf};

use crate::Desktop;

fn dirs() -> Option<(PathBuf, PathBuf)> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?
        .join("Trash");
    let (f, i) = (base.join("files"), base.join("info"));
    std::fs::create_dir_all(&f).ok()?;
    std::fs::create_dir_all(&i).ok()?;
    Some((f, i))
}

fn encode(p: &str) -> String {
    p.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// (name in the trash, original path), newest names last.
pub fn list() -> Vec<(String, String)> {
    let Some((_, info)) = dirs() else { return Vec::new() };
    let mut v: Vec<(String, String)> = std::fs::read_dir(info)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().strip_suffix(".trashinfo")?.to_string();
            let text = std::fs::read_to_string(e.path()).ok()?;
            let path = text.lines().find_map(|l| l.strip_prefix("Path="))?;
            Some((name, decode(path)))
        })
        .collect();
    v.sort();
    v
}

fn put(path: &str) -> Result<String, String> {
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err(format!("{path}: give an absolute path"));
    }
    if std::fs::symlink_metadata(p).is_err() {
        return Err(format!("{path}: no such file"));
    }
    let (files, info) = dirs().ok_or("no trash directory")?;
    let base = p.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
    let mut name = base.clone();
    let mut n = 1;
    while files.join(&name).exists() || info.join(format!("{name}.trashinfo")).exists() {
        n += 1;
        name = format!("{base}.{n}");
    }
    // info first: a trashed file without its info could never be restored
    let text = format!("[Trash Info]\nPath={}\nDeletionDate={}\n", encode(path), crate::components::widgets::now("%Y-%m-%dT%H:%M:%S"));
    std::fs::write(info.join(format!("{name}.trashinfo")), text).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(p, files.join(&name)) {
        let _ = std::fs::remove_file(info.join(format!("{name}.trashinfo")));
        return Err(format!("{path}: {e} (other filesystem? use your file manager)"));
    }
    Ok(name)
}

fn restore(name: &str) -> Result<String, String> {
    let (files, info) = dirs().ok_or("no trash directory")?;
    let meta = info.join(format!("{name}.trashinfo"));
    let text = std::fs::read_to_string(&meta).map_err(|_| format!("{name}: not in the trash"))?;
    let to = text.lines().find_map(|l| l.strip_prefix("Path=")).map(decode).ok_or("broken .trashinfo")?;
    if Path::new(&to).exists() {
        return Err(format!("{to}: already exists"));
    }
    std::fs::rename(files.join(name), &to).map_err(|e| format!("{to}: {e}"))?;
    let _ = std::fs::remove_file(meta);
    Ok(to)
}

impl Desktop {
    pub fn trash_cmd(&mut self, arg: &str) -> String {
        run(arg)
    }
}

/// The whole of `trash`: needs no compositor, so `ctl trash` and `fd-trash` work with none running.
pub fn run(arg: &str) -> String {
    {
        let arg = arg.trim();
        let r = match arg.split_once(' ') {
            _ if arg.is_empty() || arg == "list" => {
                let l = list();
                return if l.is_empty() { "(trash is empty)".into() } else { l.iter().map(|(n, p)| format!("{n}\t{p}")).collect::<Vec<_>>().join("\n") };
            }
            Some(("restore", name)) => restore(name.trim()).map(|p| format!("restored {p}")),
            _ => put(arg).map(|n| format!("trashed {n}")),
        };
        r.unwrap_or_else(|e| format!("error: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding_round_trips() {
        let p = "/home/a b/ż.txt";
        assert_eq!(decode(&encode(p)), p);
        assert_eq!(encode("/a b"), "/a%20b");
    }
}
