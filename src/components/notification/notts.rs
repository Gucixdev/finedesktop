//! `ctl notify TEXT` posts a notification: it shows in the bar (item `notify`) for a few seconds, is kept in a short
//! history (`ctl notify list`, `ctl notify clear`) and fires the `on-notify` hook, so a script decides what else
//! happens (sound, a log, forwarding). Anything that can run `ctl` can notify.

use std::{collections::VecDeque, time::{Duration, Instant}};

use crate::{components::hooks::Event, Desktop};

const KEEP: usize = 100;
/// How long the bar item stays.
pub const SHOWN: Duration = Duration::from_secs(8);

pub struct Note {
    pub id: u32,
    pub at: Instant,
    pub stamp: String,
    pub text: String,
}

#[derive(Default)]
pub struct Notes {
    pub list: VecDeque<Note>,
    next: u32,
}

impl Notes {
    pub fn push(&mut self, text: &str) -> u32 {
        self.next += 1;
        if self.list.len() == KEEP {
            self.list.pop_front();
        }
        self.list.push_back(Note { id: self.next, at: Instant::now(), stamp: crate::components::widgets::now("%H:%M:%S"), text: text.to_string() });
        self.next
    }

    /// The newest note, if it is still within `SHOWN`.
    pub fn current(&self) -> Option<&Note> {
        self.list.back().filter(|n| n.at.elapsed() < SHOWN)
    }
}

impl Desktop {
    pub fn notify_cmd(&mut self, arg: &str) -> String {
        match arg.trim() {
            "" | "list" => {
                if self.notes.list.is_empty() {
                    return "(no notifications)".into();
                }
                self.notes.list.iter().map(|n| format!("{}\t{}\t{}", n.id, n.stamp, n.text)).collect::<Vec<_>>().join("\n")
            }
            "clear" => {
                self.notes.list.clear();
                self.bar_dirty = true;
                "ok".into()
            }
            text => {
                let id = self.notes.push(text);
                self.emit(Event::notify(id, text));
                format!("ok {id}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_a_bounded_history_and_the_newest_is_current() {
        let mut n = Notes::default();
        for i in 0..(KEEP + 5) {
            n.push(&format!("m{i}"));
        }
        assert_eq!(n.list.len(), KEEP);
        assert_eq!(n.current().map(|x| x.text.as_str()), Some("m104"));
    }
}
