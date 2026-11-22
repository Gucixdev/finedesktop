//! A hierarchical state machine: the same table as `fsm`, plus a parent for every state. An event that the current
//! state has no row for bubbles up to its ancestors, and the first row found wins. The menu uses the hierarchy for
//! navigation: opening a level opens the path to it, and "back" is the parent.

use super::fsm::Fsm;

pub struct Hfsm<S: Copy + PartialEq + 'static, E: Copy + PartialEq + 'static> {
    pub fsm: Fsm<S, E>,
    parent: fn(S) -> Option<S>,
}

/// root first, ending with `s`
pub fn path_of<S: Copy>(s: S, parent: fn(S) -> Option<S>) -> Vec<S> {
    let mut v = vec![s];
    while let Some(p) = parent(*v.last().expect("not empty")) {
        v.push(p);
    }
    v.reverse();
    v
}

impl<S: Copy + PartialEq, E: Copy + PartialEq> Hfsm<S, E> {
    pub const fn new(state: S, table: &'static [(S, E, S)], parent: fn(S) -> Option<S>) -> Self {
        Self { fsm: Fsm::new(state, table), parent }
    }

    pub fn state(&self) -> S {
        self.fsm.state
    }

    /// Root first, ending with the current state.
    pub fn path(&self) -> Vec<S> {
        path_of(self.fsm.state, self.parent)
    }

    /// Is the machine in `s` or in a state below it?
    pub fn is_in(&self, s: S) -> bool {
        self.path().contains(&s)
    }

    /// Move to the parent. `None` at the root.
    pub fn up(&mut self) -> Option<S> {
        let p = (self.parent)(self.fsm.state)?;
        self.fsm.state = p;
        Some(p)
    }

    /// Handle `ev` in the current state or the nearest ancestor that has a row for it. Returns (from, to).
    pub fn fire(&mut self, ev: E) -> Option<(S, S)> {
        let from = self.fsm.state;
        let mut at = Some(from);
        while let Some(s) = at {
            if let Some(to) = self.fsm.next(s, ev) {
                self.fsm.state = to;
                return Some((from, to));
            }
            at = (self.parent)(s);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum S {
        Root,
        Settings,
        Themes,
        Closed,
    }
    #[derive(Clone, Copy, PartialEq)]
    enum E {
        Esc,
        Pick,
    }
    fn parent(s: S) -> Option<S> {
        match s {
            S::Themes => Some(S::Settings),
            S::Settings => Some(S::Root),
            _ => None,
        }
    }
    static T: [(S, E, S); 2] = [(S::Root, E::Esc, S::Closed), (S::Themes, E::Pick, S::Settings)];

    #[test]
    fn path_and_up() {
        let mut h = Hfsm::new(S::Themes, &T, parent);
        assert_eq!(h.path(), [S::Root, S::Settings, S::Themes]);
        assert!(h.is_in(S::Settings) && !h.is_in(S::Closed));
        assert_eq!(h.up(), Some(S::Settings));
        assert_eq!(h.up(), Some(S::Root));
        assert_eq!(h.up(), None);
    }

    #[test]
    fn event_bubbles_to_the_ancestor_that_handles_it() {
        let mut h = Hfsm::new(S::Themes, &T, parent);
        assert_eq!(h.fire(E::Esc), Some((S::Themes, S::Closed)));
        let mut h = Hfsm::new(S::Themes, &T, parent);
        assert_eq!(h.fire(E::Pick), Some((S::Themes, S::Settings)));
    }
}
