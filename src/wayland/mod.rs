//! Wayland protocol handling.

use _spb::wp_single_pixel_buffer_manager_v1::{self, WpSinglePixelBufferManagerV1};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::{self, WlBuffer};
use smithay_client_toolkit::reexports::client::protocol::wl_output::{Transform, WlOutput};
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::single_pixel_buffer::v1::client as _spb;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::wlr_layer::{
    LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, registry_handlers,
};

use crate::wayland::fractional_scale::{FractionalScaleHandler, FractionalScaleManager};
use crate::wayland::viewporter::Viewporter;
use crate::{Error, State};

pub mod fractional_scale;
pub mod viewporter;

/// Wayland protocol globals.
#[derive(Debug)]
pub struct ProtocolStates {
    pub single_pixel_buffer: Option<WpSinglePixelBufferManagerV1>,
    pub fractional_scale: Option<FractionalScaleManager>,
    pub compositor: CompositorState,
    pub layer_shell: LayerShell,
    pub registry: RegistryState,
    pub viewporter: Viewporter,

    output: OutputState,
}

impl ProtocolStates {
    pub fn new(globals: &GlobalList, queue: &QueueHandle<State>) -> Result<Self, Error> {
        let single_pixel_buffer = globals.bind(queue, 1..=1, ()).ok();
        let registry = RegistryState::new(globals);
        let output = OutputState::new(globals, queue);
        let layer_shell = LayerShell::bind(globals, queue)
            .map_err(|err| Error::WaylandProtocol("wlr_layer_shell", err))?;
        let compositor = CompositorState::bind(globals, queue)
            .map_err(|err| Error::WaylandProtocol("wl_compositor", err))?;
        let viewporter = Viewporter::new(globals, queue)
            .map_err(|err| Error::WaylandProtocol("wp_viewporter", err))?;
        let fractional_scale = FractionalScaleManager::new(globals, queue).ok();

        Ok(Self {
            single_pixel_buffer,
            fractional_scale,
            layer_shell,
            compositor,
            viewporter,
            registry,
            output,
        })
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
        _surface: &WlSurface,
        factor: i32,
    ) {
        if self.protocol_states.fractional_scale.is_none() {
            self.window.set_scale_factor(factor as f64);
        }
    }

    fn frame(
        &mut self,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
        _surface: &WlSurface,
        _time: u32,
    ) {
        self.window.draw();
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlSurface,
        _: Transform,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &WlSurface,
        _output: &WlOutput,
    ) {
    }
}
delegate_compositor!(State);

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.protocol_states.output
    }

    fn new_output(
        &mut self,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
        _output: WlOutput,
    ) {
    }
}
delegate_output!(State);

impl LayerShellHandler for State {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.terminated = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _queue: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.window.set_size(&self.protocol_states.compositor, configure.new_size.into());
    }
}
delegate_layer!(State);

impl FractionalScaleHandler for State {
    fn scale_factor_changed(
        &mut self,
        _connection: &Connection,
        _queue: &QueueHandle<Self>,
        _surface: &WlSurface,
        factor: f64,
    ) {
        self.window.set_scale_factor(factor);
    }
}

impl ProvidesRegistryState for State {
    registry_handlers![OutputState];

    fn registry(&mut self) -> &mut RegistryState {
        &mut self.protocol_states.registry
    }
}
delegate_registry!(State);

impl Dispatch<WpSinglePixelBufferManagerV1, ()> for State {
    fn event(
        _state: &mut State,
        _manager: &WpSinglePixelBufferManagerV1,
        _event: wp_single_pixel_buffer_manager_v1::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<State>,
    ) {
        // No events.
    }
}

impl Dispatch<WlBuffer, ()> for State {
    fn event(
        _state: &mut State,
        _buffer: &WlBuffer,
        event: wl_buffer::Event,
        _data: &(),
        _connection: &Connection,
        _queue: &QueueHandle<State>,
    ) {
        match event {
            // We never release our SPB buffers.
            wl_buffer::Event::Release => (),
            event => unreachable!("SPB event: {event:?}"),
        }
    }
}
