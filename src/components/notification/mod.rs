//! Notifications. `notts` is the store and the `ctl notify` surface; a freedesktop D-Bus daemon (`nottsd`) would be
//! a thin front for it and is not written, since it needs a D-Bus library.
pub mod notts;
