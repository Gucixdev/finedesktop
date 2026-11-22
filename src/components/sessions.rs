//! Session actions through the same surface as everything else: `ctl session lock|logout|suspend|hibernate|reboot|poweroff`.
//! The machine-level ones go to `loginctl`, so they work with systemd and elogind alike. `lock` runs the config's
//! `lock = ...` command (default `loginctl lock-session`). Each one also fires `on-user` with the name `session`.

use crate::{components::hooks::Event, config::Action, Desktop};

impl Desktop {
    pub fn session_cmd(&mut self, what: &str) -> String {
        let what = what.trim();
        let reply = match what {
            "lock" => {
                crate::keys::spawn(&self.config.lock.clone());
                "ok".to_string()
            }
            "logout" => return self.run_action(Action::Quit),
            "suspend" | "hibernate" | "reboot" | "poweroff" => {
                crate::keys::spawn(&format!("loginctl {what}"));
                "ok".to_string()
            }
            "" | "list" => return "lock logout suspend hibernate reboot poweroff save restore saved restart".into(),
            w if w.split_whitespace().next().is_some_and(|v| matches!(v, "save" | "restore" | "saved" | "restart")) => {
                return self.restore_cmd(w).unwrap_or_default();
            }
            _ => return format!("error: unknown session action {what:?} (ctl session list)"),
        };
        self.emit(Event::user("session", what));
        reply
    }
}
