//! A damped spring: the one animation primitive. `x` is pulled to `target` with `stiffness` and slowed by `damping`
//! (ratio ~0.75 by default: a small overshoot, then rest). Stepped with real frame time, so it looks the same at any fps.

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub const fn new(x: f32) -> Spring {
        Spring { x, v: 0.0, target: x, stiffness: 300.0, damping: 26.0 }
    }

    /// Advance by `dt` seconds (sub-stepped, so a long frame cannot blow the integration up).
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.2); // a stalled frame must not become one giant integration step
        let n = (dt / 0.004).ceil().clamp(1.0, 50.0);
        let h = dt / n;
        for _ in 0..n as u32 {
            let a = -self.stiffness * (self.x - self.target) - self.damping * self.v;
            self.v += a * h;
            self.x += self.v * h;
        }
    }

    pub fn settled(&self) -> bool {
        (self.x - self.target).abs() < 0.001 && self.v.abs() < 0.01
    }

    /// Jump to the target without moving through the space between.
    pub fn snap(&mut self) {
        self.x = self.target;
        self.v = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_the_target_and_rests() {
        let mut s = Spring::new(1.0);
        s.target = 0.0;
        for _ in 0..300 {
            s.step(1.0 / 60.0);
        }
        assert!(s.settled(), "x={} v={}", s.x, s.v);
    }

    #[test]
    fn a_resting_spring_stays_put() {
        let mut s = Spring::new(0.5);
        s.step(0.016);
        assert_eq!(s.x, 0.5);
        assert!(s.settled());
    }

    #[test]
    fn a_huge_frame_does_not_explode() {
        let mut s = Spring::new(1.0);
        s.target = 0.0;
        s.step(5.0);
        assert!(s.x.abs() < 2.0);
    }
}
