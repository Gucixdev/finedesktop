//! Effects: what moves on screen, as springs. Right now the menu / launcher panel (slides in and fades up); more
//! effects are more fields here and one more line in `step`.

use std::time::Instant;

use super::anim::Spring;

pub struct Fx {
    /// 1.0 when the panel has just been asked for, 0.0 when it is in place
    pub panel: Spring,
    last: Option<Instant>,
}

/// (stiffness, damping ratio) for a speed factor and a bounce amount; the defaults (1.0, 0.5) are stiffness 300, ratio 0.75.
pub fn spring_params(speed: f32, bounce: f32) -> (f32, f32) {
    let s = speed.clamp(0.25, 4.0);
    (300.0 * s * s, 1.0 - 0.5 * bounce.clamp(0.0, 1.0))
}

impl Default for Fx {
    fn default() -> Fx {
        Fx { panel: Spring::new(0.0), last: None }
    }
}

impl Fx {
    /// Start the slide-in. `on` = animations enabled (off: the panel is simply there), `speed` scales the stiffness
    /// (2 = twice as fast), `bounce` 0..1 is the overshoot (0 = none, 0.5 = the default small one, 1 = lively).
    pub fn panel_open(&mut self, on: bool, speed: f32, bounce: f32) {
        let (stiffness, ratio) = spring_params(speed, bounce);
        self.panel.stiffness = stiffness;
        self.panel.damping = 2.0 * ratio * stiffness.sqrt();
        self.panel.x = if on { 1.0 } else { 0.0 };
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
        fx.panel_open(true, 1.0, 0.5);
        assert!(fx.panel_offset() > 0.9);
        let mut t = Instant::now();
        for _ in 0..400 {
            t += Duration::from_millis(16);
            fx.step(t);
        }
        assert!(!fx.step(t + Duration::from_millis(16)));
        assert_eq!(fx.panel_offset(), 0.0);
    }

    #[test]
    fn defaults_keep_the_old_spring_and_off_means_in_place() {
        let (k, r) = spring_params(1.0, 0.5);
        assert_eq!((k, r), (300.0, 0.75));
        assert!((2.0 * r * k.sqrt() - 26.0).abs() < 0.1);
        assert!(spring_params(2.0, 0.5).0 > spring_params(1.0, 0.5).0);
        let mut fx = Fx::default();
        fx.panel_open(false, 1.0, 0.5);
        assert_eq!(fx.panel_offset(), 0.0);
    }
}
