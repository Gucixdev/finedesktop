//! The engines of finedesktop that are pure geometry: layouts and directional focus.
//! Built twice: as an rlib linked into the compositor (the fallback that always works) and as a cdylib that the
//! compositor loads with dlopen and replaces while it runs (`finedesktop ctl reload-engine`, see `hot` in the repo root).
//! The boundary is the C ABI in `abi`; change a struct there and you must bump `ABI` (and restart once).

pub mod abi;
pub mod focus_engine;
pub mod layout_engine;
