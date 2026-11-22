//! Autostart is a folder: every executable file in `<config dir>/autostart/` is started once, in name order, when the
//! desktop starts (after the `exec =` lines of the config). Drop a script in to start something, move it out to stop it.
//! `ctl autostart list` shows them, `ctl autostart run` starts them again.

use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use crate::Desktop;

pub fn files() -> Vec<PathBuf> {
    let Some(dir) = crate::config::config_dir().map(|d| d.join("autostart")) else { return Vec::new() };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .collect();
    v.sort();
    v
}

impl Desktop {
    pub fn autostart_run(&mut self) -> usize {
        let list = files();
        for f in &list {
            tracing::info!("autostart: {}", f.display());
            crate::keys::spawn(&format!("'{}'", f.display().to_string().replace('\'', "'\\''")));
        }
        list.len()
    }

    pub fn autostart_cmd(&mut self, arg: &str) -> String {
        match arg.trim() {
            "" | "list" => {
                let l = files();
                if l.is_empty() { "(nothing in the autostart folder)".into() } else { l.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n") }
            }
            "run" => format!("started {}", self.autostart_run()),
            _ => "error: autostart [list|run]".into(),
        }
    }
}
