//! C ABI between the compositor and the engine library. Plain `repr(C)` data, no Rust types cross the boundary,
//! so a library built from newer sources can replace the loaded one while windows stay open.

use crate::focus_engine::{self, Cardinal};
use crate::layout_engine::{self, BarKind, Layout, Params, Rect};

/// Bump whenever a struct or function below changes shape. The compositor refuses a library with another value.
pub const ABI: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArrangeIn {
    pub layout: u32,
    pub area: Rect,
    pub n: u32,
    pub focused: u32,
    pub nmaster: u32,
    pub mfact: f32,
    pub gap: i32,
    pub bar_h: i32,
}

/// One window of the plan: `present` 0 = leave the window alone (float).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SlotC {
    pub present: u8,
    pub visible: u8,
    pub rect: Rect,
}

/// `has_bar` 0 = no tab/stack bar; `bar_kind` 0 = tabs, 1 = rows.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ArrangeOut {
    pub has_bar: u8,
    pub bar_kind: u8,
    pub bar: Rect,
}

/// A window rectangle that may be absent (`present` 0 = not shown).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RectOpt {
    pub present: u8,
    pub rect: Rect,
}

pub const LAYOUT_COUNT: u32 = Layout::ALL.len() as u32;

pub fn layout_from(i: u32) -> Layout {
    Layout::ALL[(i as usize).min(Layout::ALL.len() - 1)]
}

pub fn layout_index(l: Layout) -> u32 {
    Layout::ALL.iter().position(|x| *x == l).unwrap_or(0) as u32
}

pub fn cardinal_from(i: u32) -> Cardinal {
    match i {
        0 => Cardinal::Left,
        1 => Cardinal::Right,
        2 => Cardinal::Up,
        _ => Cardinal::Down,
    }
}

pub fn cardinal_index(c: Cardinal) -> u32 {
    match c {
        Cardinal::Left => 0,
        Cardinal::Right => 1,
        Cardinal::Up => 2,
        Cardinal::Down => 3,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn fd_engine_abi() -> u32 {
    ABI
}

/// Fill `slots[..min(n, cap)]` and `out`; returns the number of windows planned (0 when the input is unusable).
///
/// # Safety
/// `inp` and `out` point to valid structs, `slots` to `cap` writable `SlotC`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_arrange(inp: *const ArrangeIn, slots: *mut SlotC, cap: u32, out: *mut ArrangeOut) -> u32 {
    if inp.is_null() || out.is_null() || (slots.is_null() && cap > 0) {
        return 0;
    }
    let i = unsafe { *inp };
    let p = Params { nmaster: i.nmaster as usize, mfact: i.mfact, gap: i.gap, bar_h: i.bar_h };
    let plan = layout_engine::arrange(layout_from(i.layout), i.area, (i.n as usize).min(cap as usize), i.focused as usize, &p);
    for (k, s) in plan.slots.iter().enumerate() {
        let c = match s {
            Some(s) => SlotC { present: 1, visible: s.visible as u8, rect: s.rect },
            None => SlotC { present: 0, visible: 0, rect: i.area },
        };
        unsafe { *slots.add(k) = c };
    }
    let o = match plan.bar {
        Some(b) => ArrangeOut { has_bar: 1, bar_kind: (b.kind == BarKind::Rows) as u8, bar: b.rect },
        None => ArrangeOut { has_bar: 0, bar_kind: 0, bar: i.area },
    };
    unsafe { *out = o };
    plan.slots.len() as u32
}

/// Index of the window next to `from` in direction `dir` (0 left, 1 right, 2 up, 3 down), or -1.
///
/// # Safety
/// `rects` points to `n` readable `RectOpt`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fd_neighbor(rects: *const RectOpt, n: u32, from: u32, dir: u32) -> i32 {
    if rects.is_null() {
        return -1;
    }
    let v: Vec<Option<Rect>> =
        (0..n as usize).map(|k| { let r = unsafe { *rects.add(k) }; (r.present != 0).then_some(r.rect) }).collect();
    focus_engine::neighbor(&v, from as usize, cardinal_from(dir)).map_or(-1, |i| i as i32)
}
