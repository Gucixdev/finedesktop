//! Components and their life. A component is anything with a name that wants to be told when things happen:
//!   awake    once, when it is registered            start     once, when the desktop is up (config and engine loaded)
//!   restart  `ctl restart`: config and engine reload, windows stay
//!   update   every frame, with the real frame time  fixed_update  every 20 ms, however slow the frames are
//!   tick     once a second                          enabled / disabled   `ctl enable|disable NAME`
//!   destroy  when the desktop exits
//! A disabled component gets nothing but `enabled`. Every phase is also published on the bus (`awake power`, `tick` ...);
//! scripts hear `start`, `restart`, `destroy` and `tick` once for the desktop and `enabled`/`disabled` per component
//! (`FD_NAME`); `update` and `fixupdate` only reach in-process or `ctl subscribe update` listeners (a script per frame is madness).

use crate::{components::hooks::Event, state::Desktop};

pub trait Component {
    fn name(&self) -> &'static str;
    fn awake(&mut self, _d: &mut Desktop) {}
    fn start(&mut self, _d: &mut Desktop) {}
    fn restart(&mut self, _d: &mut Desktop) {}
    fn update(&mut self, _d: &mut Desktop, _dt: f32) {}
    fn fixed_update(&mut self, _d: &mut Desktop, _dt: f32) {}
    fn tick(&mut self, _d: &mut Desktop) {}
    fn enabled(&mut self, _d: &mut Desktop) {}
    fn disabled(&mut self, _d: &mut Desktop) {}
    fn destroy(&mut self, _d: &mut Desktop) {}
}

pub struct Slot {
    pub c: Box<dyn Component>,
    pub on: bool,
    pub started: bool,
}

pub const FIXED: f32 = 0.02;

#[derive(Default)]
pub struct Lifecycle {
    pub slots: Vec<Slot>,
    pub up: bool,
    fixed_acc: f32,
    tick_acc: f32,
}

impl Desktop {
    /// Register a component: it is awake at once, and started at once if the desktop is already up.
    pub fn add_component(&mut self, c: Box<dyn Component>) {
        let name = c.name();
        self.life.slots.push(Slot { c, on: true, started: false });
        let i = self.life.slots.len() - 1;
        self.with_slot(i, |c, d| c.awake(d));
        self.emit(Event::life("awake", name));
        if self.life.up {
            self.start_slot(i);
        }
    }

    /// Run `f` on slot `i` with the slot taken out, so the component can use the whole desktop.
    fn with_slot(&mut self, i: usize, f: impl FnOnce(&mut dyn Component, &mut Desktop)) {
        let Some(slot) = self.life.slots.get_mut(i) else { return };
        let mut c = std::mem::replace(&mut slot.c, Box::new(Gone));
        f(c.as_mut(), self);
        if let Some(slot) = self.life.slots.get_mut(i) {
            slot.c = c;
        }
    }

    fn start_slot(&mut self, i: usize) {
        if self.life.slots[i].started {
            return;
        }
        self.life.slots[i].started = true;
        let name = self.life.slots[i].c.name();
        self.with_slot(i, |c, d| c.start(d));
        self.emit(Event::life("start", name));
    }

    /// Phase `f` for every enabled component, in registration order.
    fn each(&mut self, f: impl Fn(&mut dyn Component, &mut Desktop)) {
        for i in 0..self.life.slots.len() {
            if self.life.slots[i].on {
                self.with_slot(i, &f);
            }
        }
    }

    pub fn life_start(&mut self) {
        self.life.up = true;
        self.emit(Event::start());
        for i in 0..self.life.slots.len() {
            if self.life.slots[i].on {
                self.start_slot(i);
            }
        }
    }

    pub fn life_restart(&mut self) -> String {
        self.load_config();
        let engine = match self.reload_engine() {
            Ok(m) => m,
            Err(e) => format!("engine: {e}"),
        };
        for i in 0..self.life.slots.len() {
            if self.life.slots[i].on {
                let name = self.life.slots[i].c.name();
                self.with_slot(i, |c, d| c.restart(d));
                self.emit(Event::life("restart", name));
            }
        }
        self.emit(Event::restart());
        format!("ok ({engine})")
    }

    pub fn life_destroy(&mut self) {
        for i in (0..self.life.slots.len()).rev() {
            if self.life.slots[i].on {
                let name = self.life.slots[i].c.name();
                self.with_slot(i, |c, d| c.destroy(d));
                self.emit(Event::life("destroy", name));
            }
        }
        self.emit(Event::destroy());
    }

    /// Called by a timer every frame or so with the time since the last call.
    pub fn life_step(&mut self, dt: f32) {
        if !self.life.up {
            return;
        }
        let dt = dt.min(0.25);
        self.menu_hold_step(dt);
        self.draft_gc();
        self.each(|c, d| c.update(d, dt));
        if self.bus.listening("update") {
            self.emit(Event::hot("update", dt));
        }
        self.life.fixed_acc += dt;
        let mut steps = 0;
        while self.life.fixed_acc >= FIXED && steps < 5 {
            self.life.fixed_acc -= FIXED;
            steps += 1;
            self.each(|c, d| c.fixed_update(d, FIXED));
            if self.bus.listening("fixupdate") {
                self.emit(Event::hot("fixupdate", FIXED));
            }
        }
        // a long stall must not owe 50 steps per second afterwards
        self.life.fixed_acc = self.life.fixed_acc.min(FIXED);
        self.life.tick_acc += dt;
        if self.life.tick_acc >= 1.0 {
            self.life.tick_acc -= 1.0;
            self.each(|c, d| c.tick(d));
            self.emit(Event::tick());
        }
    }

    pub fn set_component(&mut self, name: &str, on: bool) -> String {
        let Some(i) = self.life.slots.iter().position(|s| s.c.name() == name) else {
            return format!("error: no component {name:?} (ctl components)");
        };
        if self.life.slots[i].on == on {
            return format!("ok ({name} was already {})", if on { "on" } else { "off" });
        }
        if on {
            self.life.slots[i].on = true;
            self.with_slot(i, |c, d| c.enabled(d));
            if self.life.up {
                self.start_slot(i);
            }
            self.emit(Event::toggled(true, name));
        } else {
            self.with_slot(i, |c, d| c.disabled(d));
            self.life.slots[i].on = false;
            self.emit(Event::toggled(false, name));
        }
        "ok".into()
    }

    pub fn components_cmd(&mut self) -> String {
        self.life.slots.iter().map(|s| format!("{} {}", s.c.name(), if s.on { "on" } else { "off" })).collect::<Vec<_>>().join("\n")
    }
}

/// Stands in for a component while it is being called.
struct Gone;
impl Component for Gone {
    fn name(&self) -> &'static str {
        "(running)"
    }
}
