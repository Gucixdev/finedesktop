//! Everything the compositor draws itself, on top of the client windows: the status bar, the tab/stack bar of
//! the monocle/tabbed/deck layouts, and window borders. Backends call `overlay()` and render the result.
//! Text goes through `font::Canvas` into an ARGB buffer that is cached per output and only redrawn when its
//! content changes.

use std::hash::{Hash, Hasher};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::{
                Id, Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
                solid::SolidColorRenderElement,
            },
            gles::GlesRenderer,
            utils::CommitCounter,
        },
    },
    output::Output,
    utils::{Physical, Point, Rectangle, Transform},
};

use super::{
    stats,
    engine::layout_engine::BarKind,
    font::{self, Canvas},
    theme::{Rgb, to_f32},
    widgets,
};
use crate::{
    Desktop,
    config::BarPos,
};

smithay::backend::renderer::element::render_elements! {
    pub Ov<=GlesRenderer>;
    Mem = MemoryRenderBufferRenderElement<GlesRenderer>,
    Solid = SolidColorRenderElement,
}

/// A picture after an item's text: a history graph (last samples, 0-100) or a fill bar (0-100).
#[derive(Debug, Clone, Hash, PartialEq)]
enum Gfx {
    None,
    Graph(Vec<u8>),
    Fill(u8),
}

#[derive(Debug, Clone, Hash)]
struct Item {
    text: String,
    fg: Rgb,
    bg: Option<Rgb>,
    gfx: Gfx,
}

fn it(text: impl Into<String>, fg: Rgb) -> Item {
    Item { text: text.into(), fg, bg: None, gfx: Gfx::None }
}

fn with_gfx(text: &str, fg: Rgb, gfx: Gfx) -> Item {
    Item { text: text.into(), fg, bg: None, gfx }
}

/// Width of the picture part, in pixels.
fn gfx_w(g: &Gfx, cw: i32, scale: i32) -> i32 {
    match g {
        Gfx::None => 0,
        Gfx::Graph(_) => widgets::HIST as i32 * 2 * scale,
        Gfx::Fill(_) => 8 * cw,
    }
}

/// Text plus gap plus picture.
fn item_w(i: &Item, cw: i32, scale: i32) -> i32 {
    let text = i.text.chars().count() as i32 * cw;
    let g = gfx_w(&i.gfx, cw, scale);
    if g == 0 { text } else { text + if text > 0 { cw / 2 } else { 0 } + g }
}

/// The picture of `i` with its left edge at `x`, spanning bar height `bh`.
fn draw_gfx(cv: &mut Canvas, i: &Item, x: i32, bh: i32, cw: i32, scale: i32, t: &super::theme::Theme) {
    let (gy, gh) = (3 * scale, bh - 6 * scale);
    let gw = gfx_w(&i.gfx, cw, scale);
    cv.rect(x, gy, gw, gh, t.border_normal);
    let hot = |v: u8| stats::level_color(v as u32, t.accent, t);
    match &i.gfx {
        Gfx::None => {}
        Gfx::Graph(h) => {
            let col = 2 * scale;
            let first = widgets::HIST - h.len().min(widgets::HIST);
            for (k, &v) in h.iter().enumerate() {
                let bar = (v as i32 * gh / 100).max(if v > 0 { 1 } else { 0 });
                cv.rect(x + (first + k) as i32 * col, gy + gh - bar, col, bar, hot(v));
            }
        }
        Gfx::Fill(v) => cv.rect(x, gy, (*v as i32).min(100) * gw / 100, gh, hot(*v)),
    }
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

impl Desktop {
    /// (left items, right items) of the status bar of monitor `mi`.
    fn bar_items(&self, mi: usize) -> (Vec<Item>, Vec<Item>) {
        let c = &self.config;
        let t = &c.theme;
        let m = &self.mons[mi];
        let sh = self.widgets.shared.lock().unwrap();
        let mut one = |name: &str, out: &mut Vec<Item>| match name {
            "tags" => {
                for (i, ws) in m.ws.iter().enumerate() {
                    // the open workspace is shown as >n<, the others as " n "
                    if i == m.cur {
                        out.push(Item { text: format!(">{}<", i + 1), fg: t.sel_fg, bg: Some(t.sel_bg), gfx: Gfx::None });
                    } else if !ws.clients.is_empty() {
                        out.push(it(format!(" {} ", i + 1), t.fg));
                    } else {
                        out.push(it(format!(" {} ", i + 1), t.dim));
                    }
                }
            }
            "layout" => out.push(it(format!("[{}]", m.ws[m.cur].layout.name()), t.accent)),
            "cpu" => out.push(it(format!("cpu {}%", sh.cpu), stats::level_color(sh.cpu, t.fg, t))),
            "ram" => out.push(it(format!("ram {}%", sh.ram), stats::level_color(sh.ram, t.fg, t))),
            "swap" => {
                if let Some(v) = sh.swap {
                    out.push(it(format!("swap {v}%"), stats::level_color(v, t.fg, t)));
                }
            }
            // history graph + the current percentage; both turn yellow from 60 % and red from 90 %
            "cpu-graph" | "ram-graph" | "gpu-graph" | "vram-graph" | "swap-graph" => {
                let key = name.trim_end_matches("-graph");
                if let Some(h) = sh.hist.get(key).filter(|h| !h.is_empty()) {
                    let v = *h.last().unwrap_or(&0) as u32;
                    out.push(with_gfx(&format!("{key} {v}%"), stats::level_color(v, t.fg, t), Gfx::Graph(h.clone())));
                }
            }
            // every real disk: name, used %, a fill bar
            "disks" => {
                for d in &sh.disks {
                    let v = d.used_pct();
                    out.push(with_gfx(&format!("{} {v}%", d.label()), stats::level_color(v, t.fg, t), Gfx::Fill(v as u8)));
                }
            }
            "disk" => {
                if let Some((_, free)) = sh.disk {
                    out.push(it(format!("disk {} free", widgets::human(free)), t.fg));
                }
            }
            "disk-bar" => {
                if let Some((total, free)) = sh.disk {
                    let used = (total - free.min(total)) * 100 / total;
                    out.push(with_gfx(&format!("disk {used}%"), stats::level_color(used as u32, t.fg, t), Gfx::Fill(used as u8)));
                }
            }
            "|" => out.push(it("|", t.dim)),
            "spk" | "vol" | "mic" => {
                let (name, v) = if name == "mic" { ("mic", sh.mic) } else { ("spk", sh.vol) };
                let (text, dim) = super::widgets::audio_label(name, v);
                out.push(it(text, if dim { t.dim } else { t.fg }));
            }
            "gpu" => {
                if let Some((g, _)) = sh.gpu {
                    out.push(it(format!("gpu {g}%"), stats::level_color(g, t.fg, t)));
                }
            }
            "vram" => {
                if let Some((_, v)) = sh.gpu {
                    out.push(it(format!("vram {v}%"), stats::level_color(v, t.fg, t)));
                }
            }
            "bat" => {
                if let Some((p, chg)) = sh.bat {
                    out.push(it(format!("bat {p}%{}", if chg { "+" } else { "" }), if p <= 15 && !chg { t.err } else { t.fg }));
                }
            }
            "notify" => {
                if let Some(n) = self.notes.current() {
                    let from = if n.app == "ctl" { String::new() } else { format!("{}: ", n.app) };
                    out.push(it(format!("! {from}{}", n.text.chars().take(60).collect::<String>()), t.err));
                } else if self.notes.dnd || self.notes.snoozed() {
                    let missed = self.notes.missed();
                    out.push(it(if missed > 0 { format!("dnd ({missed})") } else { "dnd".into() }, t.dim));
                }
            }
            "date" => out.push(it(widgets::now(&c.date_format), t.fg)),
            "clock" | "time" => out.push(it(widgets::now(&c.clock_format), t.accent)),
            other => {
                if let Some(txt) = sh.text.get(other).filter(|s| !s.is_empty()) {
                    let (fg, txt) = match txt.split_once(' ') {
                        Some((c, rest)) if c.starts_with('#') => (super::theme::parse_rgb(c).unwrap_or(t.fg), rest.to_string()),
                        _ => (t.fg, txt.clone()),
                    };
                    out.push(it(txt, fg));
                }
            }
        };
        let (mut left, mut right) = (Vec::new(), Vec::new());
        // `stats` is a shorthand for the whole set
        const STATS: [&str; 6] = ["cpu-graph", "gpu-graph", "ram-graph", "vram-graph", "swap-graph", "disks"];
        let expand = |names: &Vec<String>| -> Vec<String> { names.iter().flat_map(|n| if n == "stats" { STATS.iter().map(|s| s.to_string()).collect() } else { vec![n.clone()] }).collect() };
        for n in expand(&c.bar_left) {
            one(&n, &mut left);
        }
        for n in expand(&c.bar_right) {
            one(&n, &mut right);
        }
        (left, right)
    }

    fn cached(&mut self, key: String, w: i32, h: i32, hash: u64, draw: impl FnOnce(&mut Canvas)) -> MemoryRenderBuffer {
        if let Some((hh, buf)) = self.bar_cache.get(&key) {
            if *hh == hash {
                return buf.clone();
            }
        }
        let mut cv = Canvas::new(w, h, [0, 0, 0]);
        draw(&mut cv);
        let buf = MemoryRenderBuffer::from_slice(&cv.px, Fourcc::Argb8888, (cv.w, cv.h), 1, Transform::Normal, None);
        self.bar_cache.insert(key, (hash, buf.clone()));
        buf
    }

    fn solid_id(&mut self, key: (String, usize)) -> Id {
        self.solid_ids.entry(key).or_insert_with(Id::new).clone()
    }
}

/// Elements to draw above the windows of `output`, topmost first.
pub fn overlay(state: &mut Desktop, renderer: &mut GlesRenderer, output: &Output) -> Vec<Ov> {
    let mut out: Vec<Ov> = Vec::new();
    let Some(mi) = state.mons.iter().position(|m| m.output == *output) else { return out };
    let Some(geo) = state.space.output_geometry(output) else { return out };
    let (t, scale, bh, pos) = (state.config.theme.clone(), state.config.font_scale, state.config.bar_height(), state.config.bar);
    let name = output.name();
    let pad = 6;
    let (fcw, fch) = { let f = font::Font::current(); (f.w, f.h) };
    let ty = (bh - fch * scale) / 2;

    // window borders (inside the window rectangle), focused one in the focus colour
    if t.border > 0 {
        let focused = state.focused().map(|(_, w)| w.clone());
        let wins: Vec<_> = state.mons[mi].ws[state.mons[mi].cur].clients.clone();
        let mut n = 0;
        for w in wins {
            let Some(g) = state.space.element_geometry(&w) else { continue };
            let g: Rectangle<i32, Physical> = Rectangle::new((g.loc.x - geo.loc.x, g.loc.y - geo.loc.y).into(), (g.size.w, g.size.h).into());
            let col = if Some(&w) == focused.as_ref() && mi == state.focus_mon { t.border_focus } else { t.border_normal };
            let b = t.border;
            let edges = [
                ((g.loc.x, g.loc.y), (g.size.w, b)),
                ((g.loc.x, g.loc.y + g.size.h - b), (g.size.w, b)),
                ((g.loc.x, g.loc.y), (b, g.size.h)),
                ((g.loc.x + g.size.w - b, g.loc.y), (b, g.size.h)),
            ];
            for (p, s) in edges {
                let id = state.solid_id((name.clone(), n));
                n += 1;
                let rect = Rectangle::new(p.into(), s.into());
                let commit = CommitCounter::from(hash_of(&(p, s, col)) as usize);
                out.push(SolidColorRenderElement::new(id, rect, commit, to_f32(col), Kind::Unspecified).into());
            }
        }
    }

    // tab / stack bar of monocle, tabbed, deck ... drawn over the top of the window area
    if let Some(bar) = state.mons[mi].bar {
        let ws = &state.mons[mi].ws[state.mons[mi].cur];
        let titles: Vec<String> = ws
            .clients
            .iter()
            .map(|w| {
                let (id, _, title) = Desktop::win_info(w);
                state.names.get(&id).cloned().unwrap_or(title)
            })
            .collect();
        let focus = ws.focus;
        let (r, kind) = (bar.rect, bar.kind);
        let key = format!("{name}:tabs");
        let hash = hash_of(&(&titles, focus, r.w, r.h, t.name.clone(), scale));
        let buf = state.cached(key, r.w, r.h, hash, |cv| {
            cv.rect(0, 0, r.w, r.h, t.bar_bg);
            let n = titles.len().max(1) as i32;
            for (i, ti) in titles.iter().enumerate() {
                let (x, y, w, h) = match kind {
                    BarKind::Tabs => (i as i32 * r.w / n, 0, r.w / n, r.h),
                    BarKind::Rows => (0, i as i32 * (r.h / n), r.w, r.h / n),
                };
                let (bg, fg) = if i == focus { (t.sel_bg, t.sel_fg) } else { (t.bar_bg, t.dim) };
                cv.rect(x, y, w - 1, h, bg);
                cv.text(x + pad, y + (h - fch * scale) / 2, ti, fg, scale, x + w - 1);
            }
        });
        let at: Point<f64, Physical> = ((r.x - geo.loc.x) as f64, (r.y - geo.loc.y) as f64).into();
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, at, &buf, None, None, None, Kind::Unspecified) {
            out.push(e.into());
        }
    }

    // status bar
    if pos != BarPos::Off {
        let (left, right) = state.bar_items(mi);
        let w = geo.size.w;
        let key = format!("{name}:bar");
        let hash = hash_of(&(&left, &right, w, bh, t.name.clone(), scale));
        let buf = state.cached(key, w, bh, hash, |cv| {
            cv.rect(0, 0, w, bh, t.bar_bg);
            let cw = fcw * scale;
            let mut x = 0;
            for i in &left {
                let tw = item_w(i, cw, scale);
                let slot = tw + if i.bg.is_some() { 0 } else { pad };
                if let Some(bg) = i.bg {
                    cv.rect(x, 0, slot, bh, bg);
                }
                let tx = x + if i.bg.is_some() { 0 } else { pad / 2 };
                cv.text(tx, ty, &i.text, if i.gfx == Gfx::None { i.fg } else { t.dim }, scale, w);
                if i.gfx != Gfx::None {
                    let gx = tx + i.text.chars().count() as i32 * cw + if i.text.is_empty() { 0 } else { cw / 2 };
                    draw_gfx(cv, i, gx, bh, cw, scale, &t);
                }
                x += slot;
            }
            let mut rx = w;
            for i in right.iter().rev() {
                let tw = item_w(i, cw, scale);
                let slot = tw + if i.bg.is_some() { 0 } else { pad * 2 };
                rx -= slot;
                if let Some(bg) = i.bg {
                    cv.rect(rx, 0, slot, bh, bg);
                }
                let tx = rx + if i.bg.is_some() { 0 } else { pad };
                cv.text(tx, ty, &i.text, if i.gfx == Gfx::None { i.fg } else { t.dim }, scale, w);
                if i.gfx != Gfx::None {
                    let gx = tx + i.text.chars().count() as i32 * cw + if i.text.is_empty() { 0 } else { cw / 2 };
                    draw_gfx(cv, i, gx, bh, cw, scale, &t);
                }
            }
            cv.rect(0, if pos == BarPos::Top { bh - 1 } else { 0 }, w, 1, t.border_normal);
        });
        let y = if pos == BarPos::Top { 0 } else { geo.size.h - bh };
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (0.0, y as f64), &buf, None, None, None, Kind::Unspecified) {
            out.push(e.into());
        }
    }
    // panels: key list and markdown widgets, on empty workspaces (or when forced with `welcome show`)
    if state.panel_visible(mi) {
        if let Some((x, y, w, h)) = state.panel_rect(mi) {
            let key = format!("{name}:panels");
            let wl = &state.welcome;
            let hash = hash_of(&(&wl.docs, &wl.cur, wl.scroll, wl.sel, wl.back.len(), wl.fwd.len(), w, h, t.name.clone(), scale));
            let th = t.clone();
            let wl = std::mem::take(&mut state.welcome);
            let buf = state.cached(key, w, h, hash, |cv| {
                cv.rect(0, 0, w, h, th.bg);
                crate::components::welcome::render(&wl, cv, &th, scale);
            });
            state.welcome = wl;
            if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (x as f64, y as f64), &buf, None, None, None, Kind::Unspecified) {
                out.push(e.into());
            }
        }
    }
    // the menu: one big centred list panel above everything, on the monitor that has focus. The settings panel has
    // its tabs down the left; the panel slides in on a spring.
    if mi == state.focus_mon {
        let moving = state.fx.step(std::time::Instant::now());
        if moving {
            state.bar_dirty = true;
        }
        let off = state.fx.panel_offset();
        if let Some(menu) = state.menu.as_ref() {
            // 2x text on a big screen; the panel takes most of it, like the old dmenu setup
            let ms = if geo.size.h >= 900 { scale * 2 } else { scale };
            let lh = fch * ms;
            let tabs = menu.tabs();
            let tabw = tabs.as_ref().map_or(0, |t| (t.labels.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32 + 3) * fcw * ms);
            let wide = if tabs.is_some() { 4 } else { 3 };
            let w = (geo.size.w * wide / 5).clamp(320.min(geo.size.w), 110 * fcw * ms + tabw).min(geo.size.w - 16);
            let max_h = (geo.size.h - bh) * 4 / 5;
            let cap = ((max_h - 2 * pad) / lh - 2).max(1) as usize;
            // the right side: the open level, or (cursor on the tabs) a preview of the tab under the cursor
            let (all, sel_row) = match &tabs {
                Some(t) if t.focused => {
                    let rows = t.id.map(|id| state.menu_level(id).items.into_iter().filter(|e| !matches!(e.act, super::menu::Act::Head)).map(|e| e.row()).collect()).unwrap_or_default();
                    (rows, usize::MAX)
                }
                _ => menu.rows(),
            };
            let nrows = all.len().max(tabs.as_ref().map_or(0, |t| t.labels.len())).min(cap).max(1);
            let first = if sel_row == usize::MAX { 0 } else { (sel_row + 1).saturating_sub(nrows) };
            let shown: Vec<super::menu::Row> = all.iter().skip(first).take(nrows).cloned().collect();
            let sel = sel_row.wrapping_sub(first);
            let head = format!("{} > {}_", menu.crumbs(), menu.query);
            let count = format!("{}/{}", menu.visible().len(), menu.selectable());
            let h = (nrows as i32 + 2) * lh + 2 * pad;
            if w > 80 && h > 32 {
                let key = format!("{name}:menu");
                let hash = hash_of(&(format!("{shown:?}{tabs:?}"), sel, &head, &count, w, h, t.name.clone(), ms));
                let th = t.clone();
                let buf = state.cached(key, w, h, hash, |cv| {
                    cv.rect(0, 0, w, h, th.border_focus);
                    cv.rect(2, 2, w - 4, h - 4, th.bg);
                    cv.text(pad, pad, &head, th.accent, ms, w - pad);
                    let cw = Canvas::text_width(&count, ms);
                    cv.text(w - cw - pad, pad, &count, th.dim, ms, w);
                    if let Some(tb) = &tabs {
                        cv.rect(tabw, 2 * lh + pad, 1, h - 2 * lh - pad - 2, th.border_normal);
                        for (n, l) in tb.labels.iter().enumerate().take(nrows) {
                            let y = pad + (n as i32 + 2) * lh;
                            let on = n == tb.active;
                            if on {
                                cv.rect(2, y, tabw - 2, lh, if tb.focused { th.sel_bg } else { th.bar_bg });
                            }
                            let fg = match (on, tb.focused) {
                                (true, true) => th.sel_fg,
                                (true, false) => th.accent,
                                _ => th.dim,
                            };
                            cv.text(pad, y, l, fg, ms, tabw - 2);
                        }
                    }
                    let x0 = if tabs.is_some() { tabw + pad } else { pad };
                    for (n, r) in shown.iter().enumerate() {
                        let y = pad + (n as i32 + 2) * lh;
                        match r {
                            super::menu::Row::Head(t) => {
                                cv.text(x0, y, &format!("-- {t} --"), th.dim, ms, w - pad);
                            }
                            super::menu::Row::Check(label, on) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                let sz = lh * 2 / 3;
                                let by = y + (lh - sz) / 2;
                                cv.rect(bx, by, sz, sz, fg);
                                cv.rect(bx + 2, by + 2, sz - 4, sz - 4, if n == sel { th.sel_bg } else { th.bg });
                                if *on {
                                    cv.rect(bx + 4, by + 4, sz - 8, sz - 8, if n == sel { th.sel_fg } else { th.accent });
                                }
                                cv.text(bx + sz + pad, y, label, fg, ms, w - pad);
                            }
                            super::menu::Row::Slider(label, frac, text) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                let span = w - bx - pad;
                                let (track_x, track_w) = (bx + span * 11 / 20, span * 3 / 10);
                                let th_h = (lh / 4).max(2);
                                let ty = y + (lh - th_h) / 2;
                                cv.text(bx, y, label, fg, ms, track_x - bx - pad);
                                cv.rect(track_x, ty, track_w, th_h, th.border_normal);
                                let fill = (track_w as f32 * frac) as i32;
                                cv.rect(track_x, ty, fill, th_h, if n == sel { th.sel_fg } else { th.accent });
                                let knob = (lh / 2).max(4);
                                cv.rect(track_x + fill - knob / 2, y + (lh - knob) / 2, knob, knob, fg);
                                cv.text(track_x + track_w + pad * 2, y, text, fg, ms, w - pad);
                            }
                            super::menu::Row::Color(label, c) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel { th.sel_fg } else { th.fg };
                                let bx = x0 + if tabs.is_some() { 0 } else { pad };
                                let (sw, sh) = (lh * 2, lh * 2 / 3);
                                cv.text(bx, y, label, fg, ms, w - pad - sw - pad * 8 * ms);
                                let sx = w - pad - sw - Canvas::text_width("#rrggbb", ms) - pad;
                                cv.rect(sx - 1, y + (lh - sh) / 2 - 1, sw + 2, sh + 2, fg);
                                cv.rect(sx, y + (lh - sh) / 2, sw, sh, *c);
                                cv.text(sx + sw + pad, y, &super::theme::hex(*c), fg, ms, w);
                            }
                            super::menu::Row::Item(s) => {
                                if n == sel {
                                    cv.rect(x0 - pad + 2, y, w - x0 + pad - 4, lh, th.sel_bg);
                                }
                                let fg = if n == sel {
                                    th.sel_fg
                                } else if tabs.as_ref().is_some_and(|t| t.focused) {
                                    th.dim
                                } else {
                                    th.fg
                                };
                                cv.text(x0 + if tabs.is_some() { 0 } else { pad }, y, s, fg, ms, w - pad);
                            }
                        }
                    }
                });
                // slides down from above and fades up as the spring settles
                let slide = (off * 40.0 * ms as f32) as i32;
                let alpha = (1.0 - off).clamp(0.0, 1.0);
                let (x, y) = ((geo.size.w - w) / 2, bh + (geo.size.h - bh - h) / 2 - slide);
                if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(renderer, (x as f64, y as f64), &buf, Some(alpha), None, None, Kind::Unspecified) {
                    out.insert(0, e.into());
                }
            }
        }
    }
    out
}
