//! Application icons for the launcher: the `Icon=` name of a desktop file found as a PNG (an absolute path, the hicolor
//! theme, ~/.local/share/icons, Flatpak's exports, /usr/share/pixmaps), decoded by `png.rs`, scaled to the row height and
//! kept in a cache for the rest of the session. No SVG: an icon that exists only as SVG shows no picture.

use super::png::{self, Image};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Sizes tried in this order: the nearest above the wanted size first looks sharpest after the reduction.
const SIZES: [&str; 9] = ["48x48", "64x64", "32x32", "128x128", "256x256", "24x24", "22x22", "16x16", "512x512"];

fn roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let mut v = vec![data.join("icons/hicolor"), data.join("icons"), home.join(".icons")];
    v.push(data.join("flatpak/exports/share/icons/hicolor"));
    v.push("/var/lib/flatpak/exports/share/icons/hicolor".into());
    for d in std::env::var("XDG_DATA_DIRS").unwrap_or("/usr/local/share:/usr/share".into()).split(':').filter(|d| !d.is_empty()) {
        v.push(PathBuf::from(d).join("icons/hicolor"));
    }
    v.push("/usr/share/pixmaps".into());
    v
}

/// The PNG file for an icon name, if there is one.
pub fn find(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains("..") {
        return None;
    }
    if name.starts_with('/') {
        return name.ends_with(".png").then(|| PathBuf::from(name)).filter(|p| p.is_file());
    }
    let name = name.strip_suffix(".png").unwrap_or(name);
    for root in roots() {
        for s in SIZES {
            let p = root.join(s).join("apps").join(format!("{name}.png"));
            if p.is_file() {
                return Some(p);
            }
        }
        let p = root.join(format!("{name}.png"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

static CACHE: Mutex<Option<HashMap<(String, usize), Option<Arc<Image>>>>> = Mutex::new(None);

/// The icon `name` at `size` x `size`, None when it has no PNG or the PNG cannot be read (both remembered).
pub fn load(name: &str, size: usize) -> Option<Arc<Image>> {
    let mut g = CACHE.lock().ok()?;
    let c = g.get_or_insert_with(HashMap::new);
    c.entry((name.to_string(), size))
        .or_insert_with(|| {
            let data = std::fs::read(find(name)?).ok()?;
            png::decode(&data).ok().map(|i| Arc::new(i.scaled(size)))
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_cannot_be_files_find_nothing() {
        assert!(find("").is_none());
        assert!(find("../../etc/passwd").is_none());
        assert!(find("/etc/passwd").is_none());
        assert!(find("definitely-not-an-icon-name-xyz").is_none());
        assert!(load("definitely-not-an-icon-name-xyz", 16).is_none());
    }
}
