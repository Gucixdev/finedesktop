//! Tags are labels on windows (a window has any number, a tag is any word). A group is just "the windows with this
//! tag" plus verbs, so there is one structure and no second list to keep in step:
//!   ctl tags                      every tag and how many windows carry it
//!   ctl tags add|del TAG [WIN]    label / unlabel a window (default: the focused one); `ctl tags of [WIN]`
//!   ctl group TAG list|focus|move N|close|tag NEW
//! Rules can label on open: `rule = firefox tag=web`. Labels go when the window goes.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{components::hooks::Event, state::Desktop};

#[derive(Default)]
pub struct Tags {
    map: HashMap<u32, BTreeSet<String>>,
}

pub fn valid(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 32 && !tag.contains(char::is_whitespace)
}

impl Tags {
    pub fn add(&mut self, win: u32, tag: &str) -> bool {
        valid(tag) && self.map.entry(win).or_default().insert(tag.to_string())
    }

    pub fn del(&mut self, win: u32, tag: &str) -> bool {
        let Some(set) = self.map.get_mut(&win) else { return false };
        let had = set.remove(tag);
        if set.is_empty() {
            self.map.remove(&win);
        }
        had
    }

    pub fn of(&self, win: u32) -> Vec<String> {
        self.map.get(&win).map(|s| s.iter().cloned().collect()).unwrap_or_default()
    }

    pub fn with(&self, tag: &str) -> Vec<u32> {
        let mut v: Vec<u32> = self.map.iter().filter(|(_, s)| s.contains(tag)).map(|(w, _)| *w).collect();
        v.sort_unstable();
        v
    }

    pub fn forget(&mut self, win: u32) {
        self.map.remove(&win);
    }

    pub fn counts(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for s in self.map.values() {
            for t in s {
                *m.entry(t.clone()).or_insert(0) += 1;
            }
        }
        m
    }
}

impl Desktop {
    /// (monitor, workspace, index in workspace) of a window id.
    pub fn window_by_id(&self, id: u32) -> Option<(usize, usize, usize)> {
        for (mi, m) in self.mons.iter().enumerate() {
            for (wi, ws) in m.ws.iter().enumerate() {
                if let Some(pos) = ws.clients.iter().position(|w| Desktop::win_info(w).0 == id) {
                    return Some((mi, wi, pos));
                }
            }
        }
        None
    }

    fn focused_id(&self) -> Option<u32> {
        self.focused().map(|(_, w)| Desktop::win_info(w).0)
    }

    pub fn tag_window(&mut self, win: u32, tag: &str, on: bool) -> bool {
        let changed = if on { self.tags.add(win, tag) } else { self.tags.del(win, tag) };
        if changed {
            self.emit(Event::tagged(win, tag, on));
        }
        changed
    }

    pub fn tags_cmd(&mut self, arg: &str) -> String {
        let mut w = arg.split_whitespace();
        let verb = w.next().unwrap_or("list");
        let rest: Vec<&str> = w.collect();
        let win = |s: Option<&&str>, me: &Self| -> Result<u32, String> {
            match s {
                Some(s) => s.parse().map_err(|_| format!("error: {s:?} is not a window id (ctl state)")),
                None => me.focused_id().ok_or_else(|| "error: no focused window".to_string()),
            }
        };
        match verb {
            "list" | "" => {
                let c = self.tags.counts();
                if c.is_empty() { "(no tags)".into() } else { c.iter().map(|(t, n)| format!("{t} {n}")).collect::<Vec<_>>().join("\n") }
            }
            "of" => match win(rest.first(), self) {
                Ok(id) => self.tags.of(id).join(" "),
                Err(e) => e,
            },
            "add" | "del" => {
                let Some(tag) = rest.first() else { return "error: tags add|del TAG [WIN]".into() };
                if verb == "add" && !valid(tag) {
                    return "error: a tag is one word, at most 32 bytes".into();
                }
                match win(rest.get(1), self) {
                    Ok(id) if self.window_by_id(id).is_none() => format!("error: no window {id}"),
                    Ok(id) => if self.tag_window(id, tag, verb == "add") { "ok".into() } else { "ok (no change)".into() },
                    Err(e) => e,
                }
            }
            _ => "error: tags [list] | of [WIN] | add TAG [WIN] | del TAG [WIN]".into(),
        }
    }

    pub fn group_cmd(&mut self, arg: &str) -> String {
        let mut w = arg.split_whitespace();
        let (Some(tag), verb) = (w.next(), w.next().unwrap_or("list")) else { return "error: group TAG list|focus|move N|close|tag NEW".into() };
        let rest: Vec<&str> = w.collect();
        let members: Vec<(u32, (usize, usize, usize))> = self.tags.with(tag).into_iter().filter_map(|id| self.window_by_id(id).map(|p| (id, p))).collect();
        if members.is_empty() {
            return format!("error: no window carries {tag:?}");
        }
        match verb {
            "list" => members
                .iter()
                .map(|(id, (mi, wi, pos))| {
                    let (_, app, title) = Desktop::win_info(&self.mons[*mi].ws[*wi].clients[*pos]);
                    format!("{id} ws{} {app} {title}", wi + 1)
                })
                .collect::<Vec<_>>()
                .join("\n"),
            "focus" => {
                // the member after the focused one, wrapping: repeated calls walk through the group
                let cur = self.focused_id();
                let next = members.iter().position(|(id, _)| Some(*id) == cur).map_or(0, |i| (i + 1) % members.len());
                let (_, (mi, wi, pos)) = members[next];
                self.mons[mi].cur = wi;
                self.mons[mi].ws[wi].focus = pos;
                self.focus_mon = mi;
                self.bar_dirty = true;
                self.refocus();
                "ok".into()
            }
            "move" => {
                let Some(n) = rest.first().and_then(|n| n.parse::<usize>().ok()).filter(|n| *n >= 1) else { return "error: group TAG move N".into() };
                let mi = self.focus_mon;
                let to = (n - 1).min(self.mons[mi].ws.len() - 1);
                for (id, _) in &members {
                    if let Some((m, w, p)) = self.window_by_id(*id) {
                        let win = self.mons[m].ws[w].clients.remove(p);
                        let f = self.mons[m].ws[w].focus;
                        self.mons[m].ws[w].focus = f.min(self.mons[m].ws[w].clients.len().saturating_sub(1));
                        self.mons[mi].ws[to].clients.insert(0, win);
                        self.mons[mi].ws[to].focus = 0;
                    }
                }
                self.refocus();
                format!("ok ({} windows)", members.len())
            }
            "close" => {
                for (_, (mi, wi, pos)) in &members {
                    if let Some(t) = self.mons[*mi].ws[*wi].clients[*pos].toplevel() {
                        t.send_close();
                    }
                }
                format!("ok (asked {} windows to close)", members.len())
            }
            "tag" => {
                let Some(new) = rest.first().filter(|t| valid(t)) else { return "error: group TAG tag NEW".into() };
                for (id, _) in &members {
                    self.tag_window(*id, new, true);
                }
                "ok".into()
            }
            _ => "error: group TAG list|focus|move N|close|tag NEW".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_come_and_go() {
        let mut t = Tags::default();
        assert!(t.add(1, "web") && !t.add(1, "web") && t.add(2, "web") && t.add(2, "mail"));
        assert_eq!(t.with("web"), vec![1, 2]);
        assert_eq!(t.of(2), vec!["mail".to_string(), "web".to_string()]);
        assert!(t.del(1, "web") && !t.del(1, "web"));
        assert_eq!(t.counts().get("web"), Some(&1));
        t.forget(2);
        assert!(t.counts().is_empty());
    }

    #[test]
    fn what_a_tag_may_be() {
        assert!(valid("web") && !valid("") && !valid("two words") && !valid(&"x".repeat(33)));
        assert!(!Tags::default().add(1, "a b"));
    }
}
