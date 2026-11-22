//! flows: the few primitives the desktop is made of. Everything else is built from these and talks through them.
//!   bus        publish / subscribe / observe: every event goes through one place
//!   lifecycle  components with awake, start, restart, update, fixed update, tick, enabled, disabled, destroy
//!   tags       labels on windows; groups are "all windows with this tag" plus verbs (focus, move, close)
//!   layers     z-order names and the panels that live on them
//!   fsm, hfsm  state machines (flat, and nested with a parent path)
//! No flow knows about a particular feature; features register with them. Nothing here draws or touches a protocol.

pub mod bus;
pub mod fsm;
pub mod hfsm;
pub mod layers;
pub mod lifecycle;
pub mod tags;
