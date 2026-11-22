//! What `ctl` may do. The socket is private to the user (0600), and `ctl_deny = spawn quit session ...` in the config
//! refuses those actions when they arrive over `ctl` (agents, scripts, hooks); keys and the menu are not affected.
//! It is a guard rail against a script going too far, not a sandbox: whoever can edit the config can edit the list.

use std::{os::unix::fs::PermissionsExt, path::Path};

pub fn lock_socket(path: &Path) {
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

/// Is the first word of a ctl line on the deny list? `reload` and `subscribe` can never be denied (a bad list must stay fixable).
pub fn denied(deny: &[String], line: &str) -> bool {
    let word = line.split_whitespace().next().unwrap_or("");
    !matches!(word, "reload" | "subscribe") && deny.iter().any(|d| d == word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denies_by_first_word_but_never_reload() {
        let deny = vec!["spawn".to_string(), "reload".to_string()];
        assert!(denied(&deny, "spawn foot"));
        assert!(!denied(&deny, "layout tile"));
        assert!(!denied(&deny, "reload"));
        assert!(!denied(&deny, ""));
    }
}
