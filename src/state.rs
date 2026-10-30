use crate::backend_winit::WinitData;
use smithay::{
    desktop::{PopupManager, Space, Window},
    input::{Seat, SeatState},
    reexports::{
        calloop::{generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            Display, DisplayHandle,
        },
    },
    utils::SERIAL_COUNTER,
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        output::OutputManagerState,
        shell::xdg::XdgShellState,
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};
use std::{error::Error, ffi::OsString, process::Command, sync::Arc, time::Instant};

pub struct State {
    pub start_time: Instant,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,
    pub socket_name: OsString,

    pub space: Space<Window>,
    pub popups: PopupManager,
    pub winit: Option<WinitData>,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<State>,
    pub seat: Seat<State>,
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

impl State {
    pub fn new(
        event_loop: &mut EventLoop<'static, State>,
        display: Display<State>,
    ) -> Result<Self, Box<dyn Error>> {
        let dh = display.handle();
        let handle = event_loop.handle();

        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat: Seat<Self> = seat_state.new_wl_seat(&dh, "lumo");
        seat.add_keyboard(Default::default(), 200, 25)?;
        seat.add_pointer();

        // Accept new Wayland clients on an auto-named socket (wayland-1, ...).
        let socket = ListeningSocketSource::new_auto()?;
        let socket_name = socket.socket_name().to_os_string();
        handle
            .insert_source(socket, |stream, _, state| {
                if let Err(err) = state
                    .display_handle
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    tracing::warn!("failed to add client: {err}");
                }
            })
            .map_err(|e| e.error)?;

        // Dispatch client requests whenever the display fd is readable.
        handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // SAFETY: this is the only unsafe block in the project. `get_mut`
                    // is unsafe because the Display must not be dropped from here.
                    // We never drop it; the event source owns it for the whole run.
                    unsafe {
                        display.get_mut().dispatch_clients(state)?;
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| e.error)?;

        Ok(Self {
            start_time: Instant::now(),
            display_handle: dh,
            loop_signal: event_loop.get_signal(),
            socket_name,
            space: Space::default(),
            popups: PopupManager::default(),
            winit: None,
            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            seat,
        })
    }

    /// Ask the host window for a redraw. Called only when something changed,
    /// so an idle desktop does no rendering work.
    pub fn queue_redraw(&self) {
        if let Some(w) = &self.winit {
            w.backend.window().request_redraw();
        }
    }

    /// Give keyboard focus to `window` (or nobody) and update activated state.
    pub fn focus_window(&mut self, window: Option<Window>) {
        let serial = SERIAL_COUNTER.next_serial();
        let Some(keyboard) = self.seat.get_keyboard() else { return };

        for w in self.space.elements() {
            let _ = w.set_activated(false);
        }
        match window {
            Some(w) => {
                self.space.raise_element(&w, true);
                let _ = w.set_activated(true);
                if let Some(top) = w.toplevel() {
                    keyboard.set_focus(self, Some(top.wl_surface().clone()), serial);
                }
            }
            None => keyboard.set_focus(self, None, serial),
        }
        for w in self.space.elements() {
            if let Some(top) = w.toplevel() {
                top.send_pending_configure();
            }
        }
        self.queue_redraw();
    }

    /// Start a client program pointed at this compositor.
    /// Zombie reaping (SIGCHLD) is deferred to a later phase.
    pub fn spawn(&self, program: &str, args: &[String]) {
        match Command::new(program)
            .args(args)
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .spawn()
        {
            Ok(_) => tracing::info!("spawned {program}"),
            Err(err) => tracing::warn!("failed to spawn {program}: {err}"),
        }
    }
}
