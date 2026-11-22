//! A finite state machine as a plain table: (from, event, to). No callbacks, no allocation, so it is data you can read.

pub struct Fsm<S: Copy + PartialEq + 'static, E: Copy + PartialEq + 'static> {
    pub state: S,
    table: &'static [(S, E, S)],
}

impl<S: Copy + PartialEq, E: Copy + PartialEq> Fsm<S, E> {
    pub const fn new(state: S, table: &'static [(S, E, S)]) -> Self {
        Self { state, table }
    }

    /// The state `ev` leads to from `from`, if the table says so.
    pub fn next(&self, from: S, ev: E) -> Option<S> {
        self.table.iter().find(|(f, e, _)| *f == from && *e == ev).map(|t| t.2)
    }

    /// Take the transition from the current state. `None` = the event does not apply here.
    pub fn fire(&mut self, ev: E) -> Option<S> {
        let to = self.next(self.state, ev)?;
        self.state = to;
        Some(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum S {
        Idle,
        Rec,
    }
    #[derive(Clone, Copy, PartialEq)]
    enum E {
        Start,
        Stop,
    }
    static T: [(S, E, S); 2] = [(S::Idle, E::Start, S::Rec), (S::Rec, E::Stop, S::Idle)];

    #[test]
    fn follows_the_table_and_ignores_the_rest() {
        let mut f = Fsm::new(S::Idle, &T);
        assert_eq!(f.fire(E::Stop), None);
        assert_eq!(f.fire(E::Start), Some(S::Rec));
        assert_eq!(f.state, S::Rec);
        assert_eq!(f.fire(E::Stop), Some(S::Idle));
    }
}
