use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, KeyState, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::SERIAL_COUNTER,
};

use crate::state::Desktop;

impl Desktop {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time(&event);

                let action = self.seat.get_keyboard().unwrap().input::<crate::config::Action, _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |state, mods, handle| {
                        if event.state() != KeyState::Pressed {
                            return FilterResult::Forward;
                        }
                        let sym = handle.raw_latin_sym_or_raw_current_sym().map_or(0, |s| s.raw());
                        // XF86Switch_VT_1..12 (ctrl+alt+Fn)
                        if (0x1008FE01..=0x1008FE0C).contains(&sym) {
                            return FilterResult::Intercept(crate::config::Action::Vt((sym - 0x1008FE00) as u8));
                        }
                        match state.config.find(crate::keys::mods_of(mods), sym) {
                            Some(a) => FilterResult::Intercept(a.clone()),
                            None => FilterResult::Forward,
                        }
                    },
                );
                if let Some(a) = action {
                    self.run_action(a);
                }
            }
            InputEvent::PointerMotion { event, .. } => {
                use smithay::backend::input::PointerMotionEvent;
                let pos = self.clamp_to_outputs(self.pointer_loc + event.delta());
                self.move_pointer(pos, event.time());
                self.hover_focus();
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let Some(output) = self.mons.get(self.focus_mon).map(|m| m.output.clone()) else { return };
                let Some(geo) = self.space.output_geometry(&output) else { return };
                let pos = event.position_transformed(geo.size) + geo.loc.to_f64();
                self.move_pointer(pos, event.time());
                self.hover_focus();
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let keyboard = self.seat.get_keyboard().unwrap();

                let serial = SERIAL_COUNTER.next_serial();

                let button = event.button_code();

                let button_state = event.state();

                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    let hit = self
                        .space
                        .element_under(pointer.current_location())
                        .and_then(|(w, _)| w.toplevel().map(|t| t.wl_surface().clone()))
                        .and_then(|s| self.find_window(&s));
                    match hit {
                        Some(loc) => self.focus_loc(loc),
                        None => keyboard.set_focus(self, Option::<WlSurface>::None, serial),
                    }
                };

                pointer.button(
                    self,
                    &ButtonEvent {
                        button,
                        state: button_state,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerAxis { event, .. } => {
                let source = event.source();

                let horizontal_amount = event
                    .amount(Axis::Horizontal)
                    .unwrap_or_else(|| event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.);
                let vertical_amount = event
                    .amount(Axis::Vertical)
                    .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.);
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                let mut frame = AxisFrame::new(event.time()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, discrete as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, discrete as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
}

impl Desktop {
    pub fn clamp_to_outputs(&self, p: smithay::utils::Point<f64, smithay::utils::Logical>) -> smithay::utils::Point<f64, smithay::utils::Logical> {
        let (mut w, mut h) = (0.0f64, 0.0f64);
        for m in &self.mons {
            if let Some(g) = self.space.output_geometry(&m.output) {
                w = w.max((g.loc.x + g.size.w) as f64);
                h = h.max((g.loc.y + g.size.h) as f64);
            }
        }
        (p.x.clamp(0.0, (w - 1.0).max(0.0)), p.y.clamp(0.0, (h - 1.0).max(0.0))).into()
    }

    pub fn move_pointer(&mut self, pos: smithay::utils::Point<f64, smithay::utils::Logical>, time: smithay::backend::input::InputTime) {
        self.pointer_loc = pos;
        let serial = SERIAL_COUNTER.next_serial();
        let pointer = self.seat.get_pointer().unwrap();
        let under = self.surface_under(pos);
        pointer.motion(self, under, &MotionEvent { location: pos, serial, time });
        pointer.frame(self);
    }
}
