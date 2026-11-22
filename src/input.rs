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

                let modkey = self.modkey;
                let action = self.seat.get_keyboard().unwrap().input::<crate::keys::Action, _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |_, mods, handle| {
                        if event.state() != KeyState::Pressed {
                            return FilterResult::Forward;
                        }
                        let sym = handle.modified_sym().raw();
                        // key releases of an intercepted press are forwarded; harmless for compositor keys
                        match crate::keys::lookup(modkey, mods, sym) {
                            Some(a) => FilterResult::Intercept(a),
                            None => FilterResult::Forward,
                        }
                    },
                );
                if let Some(a) = action {
                    self.run_action(a);
                }
            }
            InputEvent::PointerMotion { .. } => {}
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();

                let output_geo = self.space.output_geometry(output).unwrap();

                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

                let serial = SERIAL_COUNTER.next_serial();

                let pointer = self.seat.get_pointer().unwrap();

                let under = self.surface_under(pos);

                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial,
                        time: event.time(),
                    },
                );
                pointer.frame(self);
            }
            InputEvent::PointerButton { event, .. } => {
                let pointer = self.seat.get_pointer().unwrap();
                let keyboard = self.seat.get_keyboard().unwrap();

                let serial = SERIAL_COUNTER.next_serial();

                let button = event.button_code();

                let button_state = event.state();

                if ButtonState::Pressed == button_state && !pointer.is_grabbed() {
                    if let Some((window, _loc)) = self
                        .space
                        .element_under(pointer.current_location())
                        .map(|(w, l)| (w.clone(), l))
                    {
                        self.space.raise_element(&window, true);
                        keyboard.set_focus(
                            self,
                            Some(window.toplevel().unwrap().wl_surface().clone()),
                            serial,
                        );
                        self.space.elements().for_each(|window| {
                            window.toplevel().unwrap().send_pending_configure();
                        });
                    } else {
                        self.space.elements().for_each(|window| {
                            window.set_activated(false);
                            window.toplevel().unwrap().send_pending_configure();
                        });
                        keyboard.set_focus(self, Option::<WlSurface>::None, serial);
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
    pub fn run_action(&mut self, a: crate::keys::Action) {
        use crate::keys::Action::*;
        let n = self.clients.len();
        let cur = self.focused_index();
        match a {
            Spawn(cmd) => crate::keys::spawn(&cmd),
            Close => {
                if let Some(t) = cur.and_then(|i| self.clients[i].toplevel()) {
                    t.send_close();
                }
            }
            Quit => self.loop_signal.stop(),
            FocusNext if n > 0 => self.focus_index(cur.map_or(0, |i| (i + 1) % n)),
            FocusPrev if n > 0 => self.focus_index(cur.map_or(0, |i| (i + n - 1) % n)),
            Zoom => {
                if let Some(i) = cur.filter(|&i| i > 0) {
                    let w = self.clients.remove(i);
                    self.clients.insert(0, w);
                    self.focus_index(0);
                }
            }
            SetLayout(l) => self.set_layout(l),
            CycleLayout(fwd) => {
                let l = if fwd { self.layout.next() } else { self.layout.prev() };
                self.set_layout(l);
            }
            Mfact(d) => {
                self.params.mfact = (self.params.mfact + d).clamp(0.1, 0.9);
                self.retile();
            }
            Nmaster(d) => {
                self.params.nmaster = (self.params.nmaster as i32 + d).max(0) as usize;
                self.retile();
            }
            FocusNext | FocusPrev => {}
        }
    }
}
