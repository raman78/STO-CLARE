//! PoC phase 2: render egui onto a wlr-layer-shell "overlay" surface.
//!
//! Combines smithay-client-toolkit (layer surface, always-on-top) with wgpu
//! (surface built from the raw wl_surface) and egui-wgpu. Draws a small sample
//! DPS table. If this renders and stays above a full-screen game, we integrate
//! the real overlay data onto this path.
//!
//! Run:  cargo run --example layershell_egui_poc     (Ctrl-C to quit)

use std::ptr::NonNull;

use egui_wgpu::wgpu;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::reexports::client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
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
    shm::{Shm, ShmHandler},
};

const WIDTH: u32 = 440;
const HEIGHT: u32 = 240;

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    layer: LayerSurface,
    conn: Connection,
    width: u32,
    height: u32,
    gpu: Option<Gpu>,
}

fn main() {
    let conn = Connection::connect_to_env().expect("no Wayland connection");
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).unwrap();
    let layer_shell = LayerShell::bind(&globals, &qh).expect("no wlr-layer-shell");
    let shm = Shm::bind(&globals, &qh).unwrap();

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

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        layer,
        conn: conn.clone(),
        width: WIDTH,
        height: HEIGHT,
        gpu: None,
    };

    println!("egui layer-shell PoC running (top-right). Launch the game; Ctrl-C to quit.");
    loop {
        event_queue.blocking_dispatch(&mut app).unwrap();
    }
}

impl App {
    fn init_gpu(&mut self) {
        let instance = wgpu::Instance::default();

        let display_ptr =
            NonNull::new(self.conn.backend().display_ptr() as *mut _).expect("null wl_display");
        let surface_ptr =
            NonNull::new(self.layer.wl_surface().id().as_ptr() as *mut _).expect("null wl_surface");
        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display_ptr));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface_ptr));

        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display),
                    raw_window_handle: raw_window,
                })
                .expect("create_surface failed")
        };

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no adapter");

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cla-overlay"),
            ..Default::default()
        }))
        .expect("no device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: self.width,
            height: self.height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let egui_renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

        self.gpu = Some(Gpu {
            device,
            queue,
            surface,
            config,
            egui_ctx: egui::Context::default(),
            egui_renderer,
        });
    }

    fn render(&mut self) {
        if self.gpu.is_none() {
            self.init_gpu();
        }
        let (w, h) = (self.width, self.height);
        let gpu = self.gpu.as_mut().unwrap();

        if gpu.config.width != w || gpu.config.height != h {
            gpu.config.width = w;
            gpu.config.height = h;
            gpu.surface.configure(&gpu.device, &gpu.config);
        }

        let ppp = 1.0;
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(w as f32, h as f32) / ppp,
            )),
            ..Default::default()
        };

        let full = gpu.egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("CLA Overlay (layer-shell PoC)");
                ui.separator();
                egui::Grid::new("dps").striped(true).show(ui, |ui| {
                    ui.label("Player");
                    ui.label("DPS");
                    ui.end_row();
                    for (name, dps) in [
                        ("Alpha", "184.2k"),
                        ("Bravo", "141.9k"),
                        ("Charlie", "97.5k"),
                    ] {
                        ui.label(name);
                        ui.label(dps);
                        ui.end_row();
                    }
                });
            });
        });

        let clipped = gpu.egui_ctx.tessellate(full.shapes, ppp);
        for (id, delta) in &full.textures_delta.set {
            gpu.egui_renderer
                .update_texture(&gpu.device, &gpu.queue, *id, delta);
        }

        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [w, h],
            pixels_per_point: ppp,
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let user_buffers = gpu.egui_renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &clipped,
            &screen,
        );

        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.02,
                                g: 0.02,
                                b: 0.02,
                                a: 0.85,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            gpu.egui_renderer.render(&mut rpass, &clipped, &screen);
        }

        gpu.queue.submit(
            user_buffers
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();

        for id in &full.textures_delta.free {
            gpu.egui_renderer.free_texture(id);
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        std::process::exit(0);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        if configure.new_size.0 != 0 {
            self.width = configure.new_size.0;
        }
        if configure.new_size.1 != 0 {
            self.height = configure.new_size.1;
        }
        self.render();
    }
}

impl CompositorHandler for App {
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
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
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

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_layer!(App);
delegate_registry!(App);
