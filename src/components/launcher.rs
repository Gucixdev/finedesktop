//! The launcher's search list (the `Id::Run` level): your apps and every executable on PATH, type to filter, Enter runs it, and a line that
//! matches nothing is run as typed. `ctl menu launch` (Super+W) opens the root list; typing there opens this list under it.

use std::{collections::BTreeSet, os::unix::fs::PermissionsExt};

/// Names of the executables on PATH, sorted, once each.
pub fn commands() -> Vec<String> {
    let mut seen = BTreeSet::new();
    for dir in std::env::var("PATH").unwrap_or_default().split(':').filter(|d| !d.is_empty()) {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let ok = std::fs::metadata(e.path()).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
            if let (true, Some(name)) = (ok, e.file_name().to_str()) {
                seen.insert(name.to_string());
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn path_has_sh() {
        assert!(super::commands().iter().any(|c| c == "sh"));
    }
}
