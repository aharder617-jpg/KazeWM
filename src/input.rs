//! Input handling. Hardcoded Ctrl+Alt bindings are TEMPORARY until Lua
//! keybindings arrive in Phase 3. (Ctrl+Alt avoids clashing with host-compositor
//! bindings that typically use Super or Alt.)

use crate::state::State;
use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent,
    },
    desktop::WindowSurfaceType,
    input::{
        keyboard::{FilterResult, Keysym},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, SERIAL_COUNTER},
};

enum Action {
    Spawn,
    Close,
    Quit,
}

impl State {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event } => {
                let Some(keyboard) = self.seat.get_keyboard() else { return };
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);
                let key_state = event.state();

                let action = keyboard.input::<Action, _>(
                    self,
                    event.key_code(),
                    key_state,
                    serial,
                    time,
                    |_, mods, handle| {
                        if key_state == KeyState::Pressed && mods.ctrl && mods.alt {
                            match handle.modified_sym() {
                                Keysym::Return => return FilterResult::Intercept(Action::Spawn),
                                Keysym::q | Keysym::Q => {
                                    return FilterResult::Intercept(Action::Close)
                                }
                                Keysym::BackSpace => return FilterResult::Intercept(Action::Quit),
                                _ => {}
                            }
                        }
                        FilterResult::Forward
                    },
                );

                match action {
                    Some(Action::Spawn) => {
                        let term = std::env::var("TERMINAL").unwrap_or_else(|_| "foot".into());
                        self.spawn(&term, &[]);
                    }
                    Some(Action::Close) => {
                        let focused = self
                            .space
                            .elements()
                            .last()
                            .and_then(|w| w.toplevel().cloned());
                        if let Some(top) = focused {
                            top.send_close();
                        }
                    }
                    Some(Action::Quit) => self.loop_signal.stop(),
                    None => {}
                }
            }

            InputEvent::PointerMotionAbsolute { event } => {
                let Some(output) = self.space.outputs().next().cloned() else { return };
                let Some(geo) = self.space.output_geometry(&output) else { return };
                let pos = event.position_transformed(geo.size) + geo.loc.to_f64();
                let Some(pointer) = self.seat.get_pointer() else { return };
                let under = self.surface_under(pos);
                pointer.motion(
                    self,
                    under,
                    &MotionEvent {
                        location: pos,
                        serial: SERIAL_COUNTER.next_serial(),
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }

            InputEvent::PointerButton { event } => {
                let Some(pointer) = self.seat.get_pointer() else { return };
                let serial = SERIAL_COUNTER.next_serial();
                let button_state = event.state();

                if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
                    let hit = self
                        .space
                        .element_under(pointer.current_location())
                        .map(|(w, _)| w.clone());
                    self.focus_window(hit);
                }

                pointer.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state: button_state,
                        serial,
                        time: event.time_msec(),
                    },
                );
                pointer.frame(self);
            }

            InputEvent::PointerAxis { event } => {
                let Some(pointer) = self.seat.get_pointer() else { return };
                let source = event.source();

                let amount = |axis: Axis| {
                    event
                        .amount(axis)
                        .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0)
                };
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let value = amount(axis);
                    if value != 0.0 {
                        frame = frame.value(axis, value);
                        if let Some(v120) = event.amount_v120(axis) {
                            frame = frame.v120(axis, v120 as i32);
                        }
                    } else if source == AxisSource::Finger {
                        frame = frame.stop(axis);
                    }
                }
                pointer.axis(self, frame);
                pointer.frame(self);
            }

            _ => {}
        }
    }

    fn surface_under(&self, pos: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        self.space.element_under(pos).and_then(|(window, location)| {
            window
                .surface_under(pos - location.to_f64(), WindowSurfaceType::ALL)
                .map(|(surface, offset)| (surface, (offset + location).to_f64()))
        })
    }
}
