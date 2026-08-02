//! Proof of concept: a wlr-layer-shell overlay surface on the "overlay" layer.
//!
//! Purpose: verify that a layer-shell surface actually stays ABOVE a
//! full-screen Proton game on this compositor (KWin). If this coloured box
//! floats over the game, the approach is validated and worth integrating egui
//! onto a layer surface. If it does not, we save ourselves the big integration.
//!
//! Run:  cargo run --example layershell_poc
//! Then launch STO (borderless or fullscreen) and check whether the box stays
//! on top. Close with Ctrl-C in the terminal.

use smithay_client_toolkit::reexports::client::{
    Connection, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 140;

fn main() {
    let conn =
        Connection::connect_to_env().expect("no Wayland connection (is this a Wayland session?)");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor not available");
    let layer_shell =
        LayerShell::bind(&globals, &qh).expect("wlr-layer-shell not available on this compositor");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm not available");

    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("sto-clare-overlay"),
        None,
    );
    layer.set_anchor(Anchor::TOP | Anchor::RIGHT);
    layer.set_size(WIDTH, HEIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let pool =
        SlotPool::new((WIDTH * HEIGHT * 4) as usize, &shm).expect("failed to create shm pool");

    let mut state = Poc {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        width: WIDTH,
        height: HEIGHT,
        configured: false,
    };

    println!(
        "layer-shell PoC running: a coloured box should appear top-right, above other windows."
    );
    println!("Launch the game and check if it stays on top. Ctrl-C to quit.");
    loop {
        event_queue.blocking_dispatch(&mut state).unwrap();
    }
}

struct Poc {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    width: u32,
    height: u32,
    configured: bool,
}

impl Poc {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let stride = self.width as i32 * 4;
        let (buffer, canvas) = self
            .pool
            .create_buffer(
                self.width as i32,
                self.height as i32,
                stride,
                wl_shm::Format::Argb8888,
            )
            .expect("failed to create buffer");

        // Semi-opaque teal so it's obvious over any background (premultiplied ARGB8888).
        for px in canvas.chunks_exact_mut(4) {
            px[0] = 0x60; // B
            px[1] = 0x90; // G
            px[2] = 0x20; // R
            px[3] = 0xC0; // A
        }

        let surface = self.layer.wl_surface();
        buffer.attach_to(surface).expect("attach failed");
        surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        surface.frame(qh, surface.clone());
        self.layer.commit();
    }
}

impl LayerShellHandler for Poc {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        std::process::exit(0);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        if configure.new_size.0 != 0 {
            self.width = configure.new_size.0;
        }
        if configure.new_size.1 != 0 {
            self.height = configure.new_size.1;
        }
        self.configured = true;
        self.draw(qh);
    }
}

impl CompositorHandler for Poc {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if self.configured {
            self.draw(qh);
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Poc {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Poc {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Poc {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(Poc);
delegate_output!(Poc);
delegate_shm!(Poc);
delegate_layer!(Poc);
delegate_registry!(Poc);
