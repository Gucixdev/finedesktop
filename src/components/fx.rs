//! Effects: what moves on screen, as springs. Right now the menu / launcher panel (slides in and fades up); more
//! effects are more fields here and one more line in `step`.

use std::time::Instant;

use super::anim::Spring;

pub struct Fx {
    /// 1.0 when the panel has just been asked for, 0.0 when it is in place
    pub panel: Spring,
    last: Option<Instant>,
}

impl Default for Fx {
    fn default() -> Fx {
        Fx { panel: Spring::new(0.0), last: None }
    }
}

impl Fx {
    pub fn panel_open(&mut self) {
        self.panel.x = 1.0;
        self.panel.v = 0.0;
        self.panel.target = 0.0;
        self.last = None;
    }

    /// Advance every effect to `now`. Returns true while something is still moving.
    pub fn step(&mut self, now: Instant) -> bool {
        let dt = self.last.map_or(0.0, |t| now.duration_since(t).as_secs_f32()).min(0.05);
        self.last = Some(now);
        if self.panel.settled() {
            self.panel.snap();
            return false;
        }
        self.panel.step(dt);
        true
    }

    /// How far the panel still has to travel: 1 = fully out, 0 = in place (may dip below 0 on the overshoot).
    pub fn panel_offset(&self) -> f32 {
        self.panel.x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn opening_starts_out_and_ends_in_place() {
        let mut fx = Fx::default();
        fx.panel_open();
        assert!(fx.panel_offset() > 0.9);
        let mut t = Instant::now();
        for _ in 0..400 {
            t += Duration::from_millis(16);
            fx.step(t);
        }
        assert!(!fx.step(t + Duration::from_millis(16)));
        assert_eq!(fx.panel_offset(), 0.0);
    }
}
