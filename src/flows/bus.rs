//! The bus. One `emit(Event)` and three kinds of listener:
//!   Stream  a `ctl subscribe [TOPIC...]` client, gets one text line per event
//!   Call    code inside the compositor (`bus.subscribe(topics, Sink::Call(..))`), gets `&mut Desktop` and the event
//!   hook    the executable `hooks/on-<event>` in the config dir (see hooks.rs); `on-event` gets every event
//! Topic = the event kind (`open`, `focus`, `tick` ...). A subscriber names topics: none or `*` = every ordinary event,
//! `NAME` = that one, `PREFIX*` = those that start so. "Hot" topics (update, fixupdate, tick) come only when named.
//! A Call listener is not called for events its own call caused (no feedback loops); streams and hooks are.
//! Observer: `Desktop::observe(name, value)` remembers the last value and publishes `change NAME VALUE` only on a change.

use std::{collections::BTreeMap, os::unix::net::UnixStream};

use crate::{components::hooks::Event, state::Desktop};

pub type Call = Box<dyn FnMut(&mut Desktop, &Event)>;

pub enum Sink {
    Stream(UnixStream),
    Call(Call),
}

pub struct Sub {
    pub id: u32,
    pub topics: Vec<String>,
    pub sink: Sink,
}

#[derive(Default)]
pub struct Bus {
    pub subs: Vec<Sub>,
    next: u32,
    seen: BTreeMap<String, String>,
}

/// Fire constantly: never delivered to `*`, never to `on-event`, and `update`/`fixupdate` never to a script at all.
pub const HOT: [&str; 3] = ["update", "fixupdate", "tick"];

pub fn is_hot(kind: &str) -> bool {
    HOT.contains(&kind)
}

pub fn wants(topics: &[String], kind: &str) -> bool {
    if topics.is_empty() {
        return !is_hot(kind);
    }
    topics.iter().any(|t| match t.as_str() {
        "*" => !is_hot(kind),
        t => t == kind || t.strip_suffix('*').is_some_and(|p| !p.is_empty() && kind.starts_with(p)),
    })
}

impl Bus {
    pub fn subscribe(&mut self, topics: Vec<String>, sink: Sink) -> u32 {
        self.next += 1;
        self.subs.push(Sub { id: self.next, topics, sink });
        self.next
    }

    pub fn unsubscribe(&mut self, id: u32) {
        self.subs.retain(|s| s.id != id);
    }

    /// Does anyone ask for this hot topic by name? (Then, and only then, it is worth building the event.)
    pub fn listening(&self, kind: &str) -> bool {
        self.subs.iter().any(|s| s.topics.iter().any(|t| wants(std::slice::from_ref(t), kind) && t != "*"))
    }

    /// Observer: the previous value when `name` changed to `value` (empty on first sight), None when it did not.
    pub fn changed(&mut self, name: &str, value: &str) -> Option<String> {
        match self.seen.get(name) {
            Some(old) if old == value => None,
            old => {
                let old = old.cloned().unwrap_or_default();
                self.seen.insert(name.to_string(), value.to_string());
                Some(old)
            }
        }
    }

    pub fn observed(&self) -> String {
        if self.seen.is_empty() {
            return "(nothing observed)".into();
        }
        self.seen.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("\n")
    }
}

impl Desktop {
    /// Publish `change NAME VALUE` if the value differs from the last one seen under NAME.
    pub fn observe(&mut self, name: &str, value: &str) -> bool {
        match self.bus.changed(name, value) {
            Some(old) => {
                self.emit(Event::change(name, &old, value));
                true
            }
            None => false,
        }
    }

    pub fn observe_cmd(&mut self, arg: &str) -> String {
        match arg.trim().split_once(' ') {
            None if arg.trim().is_empty() || arg.trim() == "list" => self.bus.observed(),
            None => "error: observe NAME VALUE | observe list".into(),
            Some((name, value)) => if self.observe(name, value.trim()) { "changed".into() } else { "same".into() },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn topics() {
        assert!(wants(&[], "open") && wants(&t(&["*"]), "open"));
        assert!(!wants(&[], "tick") && !wants(&t(&["*"]), "update"));
        assert!(wants(&t(&["tick"]), "tick") && !wants(&t(&["tick"]), "open"));
        assert!(wants(&t(&["window*"]), "window-open") && !wants(&t(&["window*"]), "open"));
        assert!(!wants(&t(&["*x"]), "ax"));
    }

    #[test]
    fn observer_reports_only_changes() {
        let mut b = Bus::default();
        assert_eq!(b.changed("bat", "80"), Some(String::new()));
        assert_eq!(b.changed("bat", "80"), None);
        assert_eq!(b.changed("bat", "79"), Some("80".into()));
        assert_eq!(b.observed(), "bat 79");
    }

    #[test]
    fn listening_needs_the_name() {
        let mut b = Bus::default();
        b.subscribe(t(&["*"]), Sink::Stream(UnixStream::pair().unwrap().0));
        assert!(!b.listening("tick"));
        b.subscribe(t(&["tick"]), Sink::Stream(UnixStream::pair().unwrap().0));
        assert!(b.listening("tick") && !b.listening("update"));
    }
}
