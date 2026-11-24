//! Where the three zones of the status bar go so they never overlap: left (`bar_left`), centre (`bar_center`, the
//! date and time) and right (`bar_right`, the statistics). Pure arithmetic over item widths, drawn by `bar`.
//!
//! Rules: the centre zone is never cut and sits in the middle of the bar; when one side needs more room than the
//! middle leaves it, the centre slides toward the side that has spare room; what still does not fit is dropped,
//! from the end of the left zone (the notification goes first) and from the inner end of the right zone, so the
//! item next to the screen edge stays.

/// What to draw: the first `left` items of the left zone, the right zone without its first `right_skip` items, and the
/// x of the centre zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub left: usize,
    pub right_skip: usize,
    pub center_x: i32,
}

fn sum(v: &[i32]) -> i32 {
    v.iter().sum()
}

/// `w` bar width, `gap` minimum space between zones, widths of the slots of each zone.
pub fn fit(w: i32, gap: i32, left: &[i32], center: &[i32], right: &[i32]) -> Fit {
    let c = sum(center);
    let (lw, rw) = (sum(left), sum(right));
    let mid = ((w - c) / 2).max(0);
    let mut cx = mid;
    // room each side has when the centre stays in the middle
    let (lroom, rroom) = (cx - gap, w - (cx + c) - gap);
    // slide toward the side with spare room when the other one is short
    if rw > rroom && lw < lroom {
        cx = (cx - (rw - rroom).min(lroom - lw)).max(0);
    } else if lw > lroom && rw < rroom {
        cx = (cx + (lw - lroom).min(rroom - rw)).min((w - c).max(0));
    }
    let (lroom, rroom) = ((cx - gap).max(0), (w - (cx + c) - gap).max(0));
    let mut left_n = left.len();
    while left_n > 0 && sum(&left[..left_n]) > lroom {
        left_n -= 1;
    }
    let mut skip = 0;
    while skip < right.len() && sum(&right[skip..]) > rroom {
        skip += 1;
    }
    Fit { left: left_n, right_skip: skip, center_x: cx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_fits_and_the_centre_is_in_the_middle() {
        let f = fit(1000, 8, &[40, 40], &[100], &[60, 60]);
        assert_eq!(f, Fit { left: 2, right_skip: 0, center_x: 450 });
    }

    #[test]
    fn long_right_zone_pushes_the_centre_left() {
        // right needs 500, the middle leaves 1000-(450+100)-8 = 442; the left only uses 80 of its 442
        let f = fit(1000, 8, &[40, 40], &[100], &[250, 250]);
        assert_eq!(f.right_skip, 0);
        assert_eq!(f.center_x, 450 - (500 - 442));
    }

    #[test]
    fn what_cannot_fit_is_dropped_from_the_inner_end_of_the_right_zone() {
        let f = fit(600, 8, &[40], &[100], &[200, 200, 200]);
        assert!(f.right_skip >= 1);
        let kept: i32 = [200, 200, 200][f.right_skip..].iter().sum();
        assert!(f.center_x + 100 + 8 + kept <= 600);
    }

    #[test]
    fn the_notification_at_the_end_of_the_left_zone_goes_first() {
        let f = fit(600, 8, &[40, 40, 400], &[100], &[60]);
        assert_eq!(f.left, 2);
    }

    #[test]
    fn zones_never_overlap_for_any_widths() {
        for w in [300, 500, 800, 1920] {
            for l in [0, 50, 300, 900] {
                for r in [0, 50, 300, 900] {
                    let f = fit(w, 8, &[l / 2, l / 2], &[120], &[r / 3, r / 3, r / 3]);
                    let lw: i32 = [l / 2, l / 2][..f.left].iter().sum();
                    let rw: i32 = [r / 3, r / 3, r / 3][f.right_skip..].iter().sum();
                    assert!(lw <= f.center_x.max(0), "left over centre: w={w} l={l} r={r}");
                    assert!(f.center_x + 120 + rw <= w.max(f.center_x + 120), "right over centre: w={w} l={l} r={r}");
                }
            }
        }
    }

    #[test]
    fn a_bar_narrower_than_the_centre_keeps_the_centre() {
        let f = fit(80, 8, &[40], &[100], &[40]);
        assert_eq!((f.left, f.right_skip, f.center_x), (0, 1, 0));
    }
}
