//! Focus and window movement by direction. Pure geometry, no compositor state, so it is unit tested:
//! `neighbor` answers "which window is to the left/right/above/below this one", `monitor_towards` the same for
//! monitors. state.rs uses them for `focus left|right|up|down`, `move left|...` and focus-follows-mouse.

use super::layout_engine::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cardinal {
    Left,
    Right,
    Up,
    Down,
}

impl Cardinal {
    pub fn parse(s: &str) -> Option<Cardinal> {
        Some(match s {
            "left" | "l" => Cardinal::Left,
            "right" | "r" => Cardinal::Right,
            "up" | "u" => Cardinal::Up,
            "down" | "d" => Cardinal::Down,
            _ => return None,
        })
    }
}

fn center(r: &Rect) -> (i64, i64) {
    ((r.x + r.w / 2) as i64, (r.y + r.h / 2) as i64)
}

/// Overlap of two 1-D spans, 0 when they do not touch.
fn overlap(a0: i32, a1: i32, b0: i32, b1: i32) -> i32 {
    (a1.min(b1) - a0.max(b0)).max(0)
}

/// Index of the nearest visible window in direction `dir` of window `from`.
/// A window counts when its centre lies past the centre of `from` in that direction. Among those the one
/// sharing the most edge with `from` wins, then the nearest; so in a master/stack tile Right from the master
/// reaches the stack window level with it, not just the closest corner.
pub fn neighbor(rects: &[Option<Rect>], from: usize, dir: Cardinal) -> Option<usize> {
    let f = (*rects.get(from)?)?;
    let (fx, fy) = center(&f);
    let mut best: Option<(usize, (i64, i64))> = None;
    for (i, r) in rects.iter().enumerate() {
        let Some(r) = r else { continue };
        if i == from {
            continue;
        }
        let (cx, cy) = center(r);
        let (along, across, shared) = match dir {
            Cardinal::Left => (fx - cx, (fy - cy).abs(), overlap(f.y, f.y + f.h, r.y, r.y + r.h)),
            Cardinal::Right => (cx - fx, (fy - cy).abs(), overlap(f.y, f.y + f.h, r.y, r.y + r.h)),
            Cardinal::Up => (fy - cy, (fx - cx).abs(), overlap(f.x, f.x + f.w, r.x, r.x + r.w)),
            Cardinal::Down => (cy - fy, (fx - cx).abs(), overlap(f.x, f.x + f.w, r.x, r.x + r.w)),
        };
        // the window must also start past the edge of `from` that faces `dir`, so a window beside or
        // around it (a stack next to a tall master) is not "above" it just because its centre is higher
        let past = match dir {
            Cardinal::Left => r.x < f.x,
            Cardinal::Right => r.x + r.w > f.x + f.w,
            Cardinal::Up => r.y < f.y,
            Cardinal::Down => r.y + r.h > f.y + f.h,
        };
        if along <= 0 || !past {
            continue;
        }
        // lower is better: no shared edge is a heavy penalty, then perpendicular offset, then distance
        let score = ((shared == 0) as i64, across * 4 + along);
        if best.is_none_or(|(_, b)| score < b) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

/// Nearest monitor in direction `dir` of monitor `from` (global geometry of each), if any lies that way.
pub fn monitor_towards(geos: &[Rect], from: usize, dir: Cardinal) -> Option<usize> {
    let rects: Vec<Option<Rect>> = geos.iter().map(|g| Some(*g)).collect();
    neighbor(&rects, from, dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    const fn r(x: i32, y: i32, w: i32, h: i32) -> Option<Rect> {
        Some(Rect { x, y, w, h })
    }

    // tile: master left, two stacked on the right
    fn tile() -> Vec<Option<Rect>> {
        vec![r(0, 0, 500, 1000), r(500, 0, 500, 500), r(500, 500, 500, 500)]
    }

    #[test]
    fn master_right_reaches_the_stack() {
        assert_eq!(neighbor(&tile(), 0, Cardinal::Right), Some(1));
    }

    #[test]
    fn stack_left_goes_to_master_and_down_to_the_next() {
        assert_eq!(neighbor(&tile(), 2, Cardinal::Left), Some(0));
        assert_eq!(neighbor(&tile(), 1, Cardinal::Down), Some(2));
        assert_eq!(neighbor(&tile(), 2, Cardinal::Up), Some(1));
    }

    #[test]
    fn nothing_beyond_the_edge() {
        assert_eq!(neighbor(&tile(), 0, Cardinal::Left), None);
        assert_eq!(neighbor(&tile(), 0, Cardinal::Up), None);
        assert_eq!(neighbor(&tile(), 2, Cardinal::Down), None);
    }

    #[test]
    fn hidden_windows_are_skipped() {
        let v = vec![r(0, 0, 500, 1000), None, r(500, 0, 500, 1000)];
        assert_eq!(neighbor(&v, 0, Cardinal::Right), Some(2));
        assert_eq!(neighbor(&v, 1, Cardinal::Right), None);
    }

    #[test]
    fn prefers_the_window_that_shares_an_edge() {
        // from is top-left; a far-right window at the same height beats a nearer one far below
        let v = vec![r(0, 0, 300, 300), r(300, 900, 300, 300), r(900, 0, 300, 300)];
        assert_eq!(neighbor(&v, 0, Cardinal::Right), Some(2));
    }

    #[test]
    fn monitors_left_to_right() {
        let g = [Rect { x: 0, y: 0, w: 1920, h: 1080 }, Rect { x: 1920, y: 0, w: 1920, h: 1080 }];
        assert_eq!(monitor_towards(&g, 0, Cardinal::Right), Some(1));
        assert_eq!(monitor_towards(&g, 1, Cardinal::Left), Some(0));
        assert_eq!(monitor_towards(&g, 0, Cardinal::Left), None);
    }
}
