//! Nested backend: the compositor is a window inside another Wayland/X11 session.

use crate::state::State;
use smithay::{
    backend::{
        egl::context::GlAttributes,
        renderer::{
            damage::OutputDamageTracker, element::surface::WaylandSurfaceRenderElement,
            gles::GlesRenderer,
        },
        winit::{self, WinitEvent, WinitGraphicsBackend},
    },
    desktop::space::render_output,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{
        calloop::EventLoop,
        winit::{dpi::LogicalSize, window::Window},
    },
    utils::Transform,
};
use std::{error::Error, time::Duration};

const CLEAR_COLOR: [f32; 4] = [0.10, 0.10, 0.12, 1.0];
const REFRESH_MHZ: i32 = 60_000;

pub struct WinitData {
    pub backend: WinitGraphicsBackend<GlesRenderer>,
    pub output: Output,
    pub damage_tracker: OutputDamageTracker,
}

pub fn init(
    event_loop: &mut EventLoop<'static, State>,
    state: &mut State,
) -> Result<(), Box<dyn Error>> {
    let attributes = Window::default_attributes()
        .with_inner_size(LogicalSize::new(1280.0, 800.0))
        .with_title("lumo");
    // Ask for GLES 2.0 so older GPUs work. Smithay's default is 3.0.
    let gl_attributes = GlAttributes {
        version: (2, 0),
        profile: None,
        debug: cfg!(debug_assertions),
        vsync: false,
    };
    let (backend, winit_source) =
        winit::init_from_attributes_with_gl_attr::<GlesRenderer>(attributes, gl_attributes)?;

    let mode = Mode { size: backend.window_size(), refresh: REFRESH_MHZ };
    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "lumo".into(),
            model: "winit".into(),
        },
    );
    let _global = output.create_global::<State>(&state.display_handle);
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);
    state.space.map_output(&output, (0, 0));

    let damage_tracker = OutputDamageTracker::from_output(&output);
    state.winit = Some(WinitData { backend, output, damage_tracker });

    event_loop
        .handle()
        .insert_source(winit_source, |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                if let Some(w) = &state.winit {
                    w.output.change_current_state(
                        Some(Mode { size, refresh: REFRESH_MHZ }),
                        None,
                        None,
                        None,
                    );
                }
                state.queue_redraw();
            }
            WinitEvent::Input(event) => state.process_input_event(event),
            WinitEvent::Redraw => {
                if let Err(err) = redraw(state) {
                    tracing::error!("render failed: {err}");
                }
            }
            WinitEvent::CloseRequested => state.loop_signal.stop(),
            _ => {}
        })
        .map_err(|e| e.error)?;

    Ok(())
}

fn redraw(state: &mut State) -> Result<(), Box<dyn Error>> {
    // Temporarily take the backend out of State so we can borrow both freely.
    let Some(mut w) = state.winit.take() else { return Ok(()) };
    let result = render(state, &mut w);
    state.winit = Some(w);
    result
}

fn render(state: &mut State, w: &mut WinitData) -> Result<(), Box<dyn Error>> {
    let age = w.backend.buffer_age().unwrap_or(0);

    let damage = {
        let (renderer, mut framebuffer) = w.backend.bind()?;
        let result = render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
            &w.output,
            renderer,
            &mut framebuffer,
            1.0,
            age,
            [&state.space],
            &[],
            &mut w.damage_tracker,
            CLEAR_COLOR,
        )?;
        result.damage.map(|d| d.to_vec())
    };

    // Only swap buffers if something actually changed on screen.
    if let Some(damage) = damage {
        w.backend.submit(Some(&damage))?;
    }

    // Always answer frame callbacks so clients keep rendering.
    let elapsed = state.start_time.elapsed();
    let output = &w.output;
    for window in state.space.elements() {
        window.send_frame(output, elapsed, Some(Duration::ZERO), |_, _| Some(output.clone()));
    }

    state.space.refresh();
    state.popups.cleanup();
    state.display_handle.flush_clients()?;
    Ok(())
}
