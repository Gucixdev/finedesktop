//! Layout engine: pure geometry, no smithay types, fully unit-tested.
//!
//! Seven classic layouts (same set as the old C desktop):
//!   tile     master column left (`nmaster`, `mfact`), the rest stacked right (dwm tile)
//!   float    windows keep their own position and size
//!   monocle  every window fills the area, only the focused one is shown
//!   deck     master column left, right column shows only one window of the stack
//!   grid     rows x cols, equal cells
//!   stack    one window shown, one title row per window above it (like i3 stacking)
//!   tab      one window shown, one tab bar above it (like i3 tabbed)
//!
//! `arrange` returns a `Plan`; the compositor applies it (configure + map/unmap) and,
//! later, draws `Plan::bar`. New layouts = one new `Layout` variant + one `match` arm.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Tile,
    Float,
    Monocle,
    Deck,
    Grid,
    Stack,
    Tab,
    Spiral,
}

impl Layout {
    pub const ALL: [Layout; 8] = [
        Layout::Tile,
        Layout::Float,
        Layout::Monocle,
        Layout::Deck,
        Layout::Grid,
        Layout::Stack,
        Layout::Tab,
        Layout::Spiral,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Layout::Tile => "tile",
            Layout::Float => "float",
            Layout::Monocle => "monocle",
            Layout::Deck => "deck",
            Layout::Grid => "grid",
            Layout::Stack => "stack",
            Layout::Tab => "tab",
            Layout::Spiral => "spiral",
        }
    }

    pub fn from_name(s: &str) -> Option<Layout> {
        // his words: "tiled" means equal cells (grid), "fibonacci" the spiral; the dwm column layout stays `tile`
        let s = match s {
            "tiled" => "grid",
            "fibonacci" => "spiral",
            other => other,
        };
        Layout::ALL.into_iter().find(|l| l.name() == s)
    }

    pub fn next(self) -> Layout {
        let i = Layout::ALL.iter().position(|l| *l == self).unwrap_or(0);
        Layout::ALL[(i + 1) % Layout::ALL.len()]
    }

    pub fn prev(self) -> Layout {
        let i = Layout::ALL.iter().position(|l| *l == self).unwrap_or(0);
        Layout::ALL[(i + Layout::ALL.len() - 1) % Layout::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub nmaster: usize,
    pub mfact: f32,
    pub gap: i32,
    /// Height of one tab / title row in px. 0 makes tab and stack behave like monocle.
    pub bar_h: i32,
}

impl Default for Params {
    fn default() -> Self {
        Params { nmaster: 1, mfact: 0.55, gap: 0, bar_h: 22 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub rect: Rect,
    pub visible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarKind {
    /// Tabs side by side in one row.
    Tabs,
    /// One title row per window, stacked vertically.
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub rect: Rect,
    pub kind: BarKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// One entry per window, same order as the client list. `None` = leave the window alone (float).
    pub slots: Vec<Option<Slot>>,
    pub bar: Option<Bar>,
}

pub fn arrange(layout: Layout, area: Rect, n: usize, focused: usize, p: &Params) -> Plan {
    let focused = if n == 0 { 0 } else { focused.min(n - 1) };
    let shown = |rects: Vec<Rect>| Plan {
        slots: rects.into_iter().map(|rect| Some(Slot { rect, visible: true })).collect(),
        bar: None,
    };
    match layout {
        Layout::Float => Plan { slots: vec![None; n], bar: None },
        Layout::Tile => shown(tile(area, n, p)),
        Layout::Grid => shown(grid(area, n, p.gap)),
        Layout::Spiral => shown(spiral(area, n, p)),
        Layout::Monocle => one_visible(area, n, focused, p.gap, None),
        Layout::Deck => deck(area, n, focused, p),
        Layout::Tab => bar_layout(area, n, focused, p, BarKind::Tabs),
        Layout::Stack => bar_layout(area, n, focused, p, BarKind::Rows),
    }
}

fn pad(r: Rect, gap: i32) -> Rect {
    Rect { x: r.x + gap, y: r.y + gap, w: (r.w - 2 * gap).max(1), h: (r.h - 2 * gap).max(1) }
}

/// Master column left, stack column right.
fn tile(area: Rect, n: usize, p: &Params) -> Vec<Rect> {
    let nmaster = p.nmaster.min(n);
    let mw = master_width(area.w, nmaster, n - nmaster, p.mfact);
    let mut out = Vec::with_capacity(n);
    column(&mut out, Rect { w: mw, ..area }, nmaster, p.gap);
    column(&mut out, Rect { x: area.x + mw, w: area.w - mw, ..area }, n - nmaster, p.gap);
    out
}

/// Fibonacci spiral: window i takes `mfact` of what is left, cutting from the left, top, right, bottom in turn, so
/// the cells wind inwards; the last window gets the remainder. mfact 0.62 is the golden spiral.
fn spiral(area: Rect, n: usize, p: &Params) -> Vec<Rect> {
    let f = p.mfact.clamp(0.1, 0.9);
    let mut rest = area;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        if i == n - 1 {
            out.push(pad(rest, p.gap));
            break;
        }
        let (cell, left) = match i % 4 {
            0 => {
                let w = (rest.w as f32 * f) as i32;
                (Rect { w, ..rest }, Rect { x: rest.x + w, w: rest.w - w, ..rest })
            }
            1 => {
                let h = (rest.h as f32 * f) as i32;
                (Rect { h, ..rest }, Rect { y: rest.y + h, h: rest.h - h, ..rest })
            }
            2 => {
                let w = (rest.w as f32 * f) as i32;
                (Rect { x: rest.x + rest.w - w, w, ..rest }, Rect { w: rest.w - w, ..rest })
            }
            _ => {
                let h = (rest.h as f32 * f) as i32;
                (Rect { y: rest.y + rest.h - h, h, ..rest }, Rect { h: rest.h - h, ..rest })
            }
        };
        out.push(pad(cell, p.gap));
        rest = left;
    }
    out
}

fn master_width(width: i32, nmaster: usize, nstack: usize, mfact: f32) -> i32 {
    if nmaster == 0 {
        0
    } else if nstack == 0 {
        width
    } else {
        (width as f32 * mfact.clamp(0.1, 0.9)) as i32
    }
}

/// Stack `count` cells vertically inside `col`; integer rounding is absorbed evenly.
fn column(out: &mut Vec<Rect>, col: Rect, count: usize, gap: i32) {
    let mut y = col.y;
    for i in 0..count {
        let h = (col.y + col.h - y) / (count - i) as i32;
        out.push(pad(Rect { x: col.x, y, w: col.w, h }, gap));
        y += h;
    }
}

fn grid(area: Rect, n: usize, gap: i32) -> Vec<Rect> {
    if n == 0 {
        return Vec::new();
    }
    let cols = (1..=n).find(|c| c * c >= n).unwrap_or(1);
    let rows = n.div_ceil(cols);
    let mut out = Vec::with_capacity(n);
    let mut y = area.y;
    for r in 0..rows {
        let h = (area.y + area.h - y) / (rows - r) as i32;
        let in_row = if r == rows - 1 { n - cols * (rows - 1) } else { cols };
        let mut x = area.x;
        for c in 0..in_row {
            let w = (area.x + area.w - x) / (in_row - c) as i32;
            out.push(pad(Rect { x, y, w, h }, gap));
            x += w;
        }
        y += h;
    }
    out
}

fn one_visible(area: Rect, n: usize, focused: usize, gap: i32, bar: Option<Bar>) -> Plan {
    Plan {
        slots: (0..n).map(|i| Some(Slot { rect: pad(area, gap), visible: i == focused })).collect(),
        bar,
    }
}

fn deck(area: Rect, n: usize, focused: usize, p: &Params) -> Plan {
    let nmaster = p.nmaster.min(n);
    if nmaster == 0 {
        return one_visible(area, n, focused, p.gap, None);
    }
    if n <= nmaster {
        return Plan { slots: tile(area, n, p).into_iter().map(|rect| Some(Slot { rect, visible: true })).collect(), bar: None };
    }
    let mw = master_width(area.w, nmaster, n - nmaster, p.mfact);
    let mut rects = Vec::with_capacity(n);
    column(&mut rects, Rect { w: mw, ..area }, nmaster, p.gap);
    let stack_area = pad(Rect { x: area.x + mw, w: area.w - mw, ..area }, p.gap);
    let top = focused.max(nmaster);
    let mut slots: Vec<Option<Slot>> =
        rects.into_iter().map(|rect| Some(Slot { rect, visible: true })).collect();
    for i in nmaster..n {
        slots.push(Some(Slot { rect: stack_area, visible: i == top }));
    }
    Plan { slots, bar: None }
}

fn bar_layout(area: Rect, n: usize, focused: usize, p: &Params, kind: BarKind) -> Plan {
    if p.bar_h <= 0 || n == 0 {
        return one_visible(area, n, focused, p.gap, None);
    }
    let rows = match kind {
        BarKind::Tabs => 1,
        BarKind::Rows => n as i32,
    };
    let bar_h = (p.bar_h * rows).min(area.h - 1);
    let bar = Bar { rect: Rect { h: bar_h, ..area }, kind };
    let body = Rect { y: area.y + bar_h, h: area.h - bar_h, ..area };
    one_visible(body, n, focused, p.gap, Some(bar))
}

#[cfg(test)]
mod tests {
    use super::*;
    const A: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };
    fn p() -> Params {
        Params { mfact: 0.5, bar_h: 20, ..Params::default() }
    }
    fn rects(plan: &Plan) -> Vec<Rect> {
        plan.slots.iter().map(|s| s.unwrap().rect).collect()
    }
    fn visible(plan: &Plan) -> Vec<bool> {
        plan.slots.iter().map(|s| s.unwrap().visible).collect()
    }

    #[test]
    fn empty_for_every_layout() {
        for l in Layout::ALL {
            assert!(arrange(l, A, 0, 0, &p()).slots.is_empty(), "{l:?}");
        }
    }

    #[test]
    fn aliases_use_his_words() {
        assert_eq!(Layout::from_name("tiled"), Some(Layout::Grid));
        assert_eq!(Layout::from_name("fibonacci"), Some(Layout::Spiral));
        assert_eq!(Layout::from_name("tile"), Some(Layout::Tile));
    }

    #[test]
    fn spiral_winds_inwards_and_covers_the_area() {
        let params = Params { gap: 0, mfact: 0.5, ..p() };
        let r = rects(&arrange(Layout::Spiral, A, 5, 0, &params));
        assert_eq!(r.len(), 5);
        assert_eq!(r[0], Rect { x: 0, y: 0, w: A.w / 2, h: A.h }); // left half
        assert_eq!(r[1], Rect { x: A.w / 2, y: 0, w: A.w / 2, h: A.h / 2 }); // then top of the rest
        let area: i64 = r.iter().map(|c| c.w as i64 * c.h as i64).sum();
        assert_eq!(area, A.w as i64 * A.h as i64, "cells tile the area exactly");
        for (i, a) in r.iter().enumerate() {
            for b in &r[i + 1..] {
                assert!(a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn tile_single_fills_screen() {
        assert_eq!(rects(&arrange(Layout::Tile, A, 1, 0, &p())), vec![A]);
    }

    #[test]
    fn tile_two_columns() {
        let r = rects(&arrange(Layout::Tile, A, 2, 0, &p()));
        assert_eq!(r[0], Rect { x: 0, y: 0, w: 960, h: 1080 });
        assert_eq!(r[1], Rect { x: 960, y: 0, w: 960, h: 1080 });
    }

    #[test]
    fn tile_stack_covers_height_without_overlap() {
        let r = rects(&arrange(Layout::Tile, A, 5, 0, &p()));
        let stack = &r[1..];
        assert_eq!(stack.iter().map(|t| t.h).sum::<i32>(), 1080);
        for pair in stack.windows(2) {
            assert_eq!(pair[0].y + pair[0].h, pair[1].y);
        }
    }

    #[test]
    fn tile_all_master_when_nmaster_exceeds_n() {
        let params = Params { nmaster: 5, ..p() };
        assert!(rects(&arrange(Layout::Tile, A, 2, 0, &params)).iter().all(|t| t.w == 1920));
    }

    #[test]
    fn float_leaves_windows_alone() {
        let plan = arrange(Layout::Float, A, 3, 0, &p());
        assert!(plan.slots.iter().all(|s| s.is_none()));
    }

    #[test]
    fn monocle_shows_only_focused() {
        let plan = arrange(Layout::Monocle, A, 3, 1, &p());
        assert_eq!(visible(&plan), vec![false, true, false]);
        assert!(rects(&plan).iter().all(|r| *r == A));
    }

    #[test]
    fn deck_shows_one_stack_window() {
        let plan = arrange(Layout::Deck, A, 4, 3, &p());
        assert_eq!(visible(&plan), vec![true, false, false, true]);
        let plan = arrange(Layout::Deck, A, 4, 0, &p());
        assert_eq!(visible(&plan), vec![true, true, false, false]);
    }

    #[test]
    fn grid_cells_cover_area() {
        for n in 1..=12 {
            let area: i32 = rects(&arrange(Layout::Grid, A, n, 0, &p())).iter().map(|r| r.w * r.h).sum();
            assert_eq!(area, 1920 * 1080, "n={n}");
        }
    }

    #[test]
    fn tab_has_one_row_bar() {
        let plan = arrange(Layout::Tab, A, 3, 2, &p());
        let bar = plan.bar.unwrap();
        assert_eq!((bar.kind, bar.rect.h), (BarKind::Tabs, 20));
        assert_eq!(visible(&plan), vec![false, false, true]);
        assert_eq!(rects(&plan)[2], Rect { x: 0, y: 20, w: 1920, h: 1060 });
    }

    #[test]
    fn stack_bar_grows_with_window_count() {
        let plan = arrange(Layout::Stack, A, 4, 0, &p());
        assert_eq!(plan.bar.unwrap().rect.h, 80);
        assert_eq!(rects(&plan)[0].y, 80);
    }

    #[test]
    fn zero_bar_height_behaves_like_monocle() {
        let params = Params { bar_h: 0, ..p() };
        let plan = arrange(Layout::Tab, A, 2, 0, &params);
        assert!(plan.bar.is_none());
        assert_eq!(rects(&plan)[0], A);
    }

    #[test]
    fn names_roundtrip_and_cycle() {
        for l in Layout::ALL {
            assert_eq!(Layout::from_name(l.name()), Some(l));
            assert_eq!(l.next().prev(), l);
        }
    }
}
