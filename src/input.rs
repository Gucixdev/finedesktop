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

/// What a key does in the menu, as a `ctl menu` command.
fn menu_key(sym: smithay::input::keyboard::Keysym, ctrl: bool) -> String {
    let c = sym.key_char();
    if ctrl {
        return match c {
            Some('n') | Some('j') => "key down",
            Some('p') | Some('k') => "key up",
            Some('g') | Some('c') => "key esc",
            Some('h') => "key backspace",
            Some('m') => "key enter",
            _ => "noop",
        }
        .into();
    }
    match sym.raw() {
        0xff1b => "key esc".into(),
        0xff0d | 0xff8d => "key enter".into(),
        0xff08 => "key backspace".into(),
        0xff51 => "key left".into(),
        0xff53 => "key right".into(),
        0xff52 => "key up".into(),
        0xff54 | 0xff09 => "key down".into(),
        0xff55 => "key pageup".into(),
        0xff56 => "key pagedown".into(),
        0x20 => "key space".into(),
        _ => match c {
            Some(ch) if !ch.is_control() => format!("type {ch}"),
            _ => "noop".into(),
        },
    }
}

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
                        // while the menu is open it takes every key (Super+binds still work, so Super+W closes it)
                        if state.menu.is_some() && !mods.logo {
                            if event.state() != KeyState::Pressed {
                                return FilterResult::Intercept(crate::config::Action::Menu("noop".into()));
                            }
                            return FilterResult::Intercept(crate::config::Action::Menu(menu_key(handle.modified_sym(), mods.ctrl)));
                        }
                        if event.state() != KeyState::Pressed {
                            return FilterResult::Forward;
                        }
                        let sym = handle.raw_latin_sym_or_raw_current_sym().map_or(0, |s| s.raw());
                        // the welcome widget reads plain keys when it is showing and no window has focus
                        if !mods.logo && !mods.ctrl && !mods.alt && state.welcome_active() {
                            if let Some(c) = crate::components::welcome::key_cmd(handle.modified_sym().raw(), mods.shift) {
                                return FilterResult::Intercept(crate::config::Action::Panel(c.into()));
                            }
                        }
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
                        None => {
                            // a left click on the welcome widget (page links, back, forward) is its own
                            if button == 0x110 && self.welcome_click(pointer.current_location()) {
                                return;
                            }
                            keyboard.set_focus(self, Option::<WlSurface>::None, serial)
                        }
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

#[cfg(test)]
mod tests {
    use super::menu_key;
    use smithay::input::keyboard::Keysym;

    #[test]
    fn menu_keys_map_to_ctl_commands() {
        let k = |raw, ctrl| menu_key(Keysym::new(raw), ctrl);
        assert_eq!(k(0xff0d, false), "key enter");
        assert_eq!(k(0xff1b, false), "key esc");
        assert_eq!(k(0xff08, false), "key backspace");
        assert_eq!(k(0x20, false), "key space");
        assert_eq!(k(0x61, false), "type a");
        assert_eq!(k(0x41, false), "type A");
        assert_eq!(k(0x6e, true), "key down");
        assert_eq!(k(0x70, true), "key up");
        assert_eq!(k(0xffe1, false), "noop"); // shift alone types nothing
    }
}
