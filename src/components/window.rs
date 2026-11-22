//! Window rules: `rule = APP_ID key=value ...` in the config. A window whose app id matches (exact, case-insensitive;
//! a trailing `*` matches a prefix) gets the keys once, when its app id first becomes known:
//!   ws=N      start on workspace N (of the monitor it opened on)
//!   name=TEXT the tab/stack label (no spaces; see `ctl name`)
//!   tag=a,b   labels (flows/tags.rs; `ctl group a focus` works on them)
//! Example: `rule = firefox ws=2 name=web`.

use crate::Desktop;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Rule {
    pub app: String,
    pub ws: Option<usize>,
    pub name: Option<String>,
    pub tags: Vec<String>,
}

impl Rule {
    pub fn parse(val: &str) -> Option<Rule> {
        let mut it = val.split_whitespace();
        let mut r = Rule { app: it.next()?.to_lowercase(), ..Rule::default() };
        for kv in it {
            match kv.split_once('=')? {
                ("ws", n) => r.ws = Some(n.parse::<usize>().ok().filter(|n| *n >= 1)?),
                ("name", t) => r.name = Some(t.to_string()),
                ("tag", t) => r.tags.extend(t.split(',').filter(|t| crate::flows::tags::valid(t)).map(String::from)),
                _ => return None,
            }
        }
        Some(r)
    }

    pub fn matches(&self, app: &str) -> bool {
        let app = app.to_lowercase();
        match self.app.strip_suffix('*') {
            Some(prefix) => app.starts_with(prefix),
            None => self.app == app,
        }
    }
}

impl Desktop {
    /// Apply the rules to windows that have an app id now and were not handled yet.
    pub fn apply_rules(&mut self) {
        if self.config.rules.is_empty() {
            return;
        }
        let mut todo = Vec::new();
        for (mi, m) in self.mons.iter().enumerate() {
            for (wi, ws) in m.ws.iter().enumerate() {
                for w in &ws.clients {
                    let (id, app, _) = Desktop::win_info(w);
                    if !app.is_empty() && !self.ruled.contains(&id) {
                        todo.push((id, app, mi, wi));
                    }
                }
            }
        }
        for (id, app, mi, from) in todo {
            self.ruled.insert(id);
            let Some(rule) = self.config.rules.iter().find(|r| r.matches(&app)).cloned() else { continue };
            if let Some(n) = &rule.name {
                self.names.insert(id, n.clone());
            }
            for t in &rule.tags {
                self.tag_window(id, t, true);
            }
            let m = &mut self.mons[mi];
            if let Some(to) = rule.ws.map(|n| (n - 1).min(m.ws.len() - 1)).filter(|to| *to != from) {
                if let Some(pos) = m.ws[from].clients.iter().position(|w| Desktop::win_info(w).0 == id) {
                    let w = m.ws[from].clients.remove(pos);
                    m.ws[from].focus = m.ws[from].focus.min(m.ws[from].clients.len().saturating_sub(1));
                    m.ws[to].clients.insert(0, w);
                    m.ws[to].focus = 0;
                }
            }
            self.bar_dirty = true;
            self.refocus();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_matches() {
        let r = Rule::parse("Firefox ws=2 name=web tag=a,b").unwrap();
        assert_eq!((r.ws, r.name.as_deref()), (Some(2), Some("web")));
        assert_eq!(r.tags, vec!["a".to_string(), "b".to_string()]);
        assert!(r.matches("firefox") && r.matches("FIREFOX") && !r.matches("firefox-esr"));
        assert!(Rule::parse("org.gnome.*").unwrap().matches("org.gnome.Nautilus"));
        assert!(Rule::parse("x ws=0").is_none() && Rule::parse("x color=red").is_none());
    }
}
