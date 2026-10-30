//! lumo: Phase 1 prototype.
//! Nested Wayland compositor (runs as a window inside another session such as MangoWC).

mod backend_winit;
mod handlers;
mod input;
mod state;

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use state::State;
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut event_loop: EventLoop<'static, State> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let mut state = State::new(&mut event_loop, display)?;

    backend_winit::init(&mut event_loop, &mut state)?;
    state.queue_redraw();

    // Optional: `lumo foot` starts a client at launch. This is an explicit
    // user request on the command line, never implicit.
    let mut args = std::env::args().skip(1);
    if let Some(program) = args.next() {
        let rest: Vec<String> = args.collect();
        state.spawn(&program, &rest);
    }

    tracing::info!("listening on WAYLAND_DISPLAY={:?}", state.socket_name);

    // `None` timeout: the loop sleeps until an event arrives (no busy loop).
    event_loop.run(None, &mut state, |state| {
        let _ = state.display_handle.flush_clients();
    })?;

    Ok(())
}
