//! Wayland layer-shell overlay (Linux).
//!
//! On Wayland a normal (winit) window cannot force itself above a full-screen
//! game — the always-on-top hint is ignored. The `wlr-layer-shell` "overlay"
//! layer *is* honored by the compositor (KWin included), so the overlay runs
//! here as a layer surface rendered with egui via wgpu, on its own thread, fed
//! data from the main app over a calloop channel.
//!
//! Public API: [`LayerOverlay::spawn`], [`LayerOverlay::update`],
//! [`LayerOverlay::stop`]. The struct stops the thread on drop.

use std::ptr::NonNull;
use std::thread::JoinHandle;

use egui_wgpu::wgpu;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{Shm, ShmHandler},
};
use smithay_client_toolkit::reexports::calloop::{
    channel::{channel, Channel, Event as ChannelEvent, Sender},
    EventLoop,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, Proxy, QueueHandle,
};

/// Snapshot of what the overlay should display. Plain, `Send` data.
#[derive(Clone, Default)]
pub struct OverlayData {
    pub columns: Vec<String>,
    pub rows: Vec<OverlayRow>,
}

#[derive(Clone)]
pub struct OverlayRow {
    pub name: String,
    pub values: Vec<String>,
}

enum Msg {
    Data(OverlayData),
    Stop,
}

/// Handle to the overlay thread held by the main app.
pub struct LayerOverlay {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

impl LayerOverlay {
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<Msg>();
        let join = std::thread::Builder::new()
            .name("cla-layer-overlay".into())
            .spawn(move || {
                if let Err(e) = run(rx) {
                    log::error!("layer overlay: {e}");
                }
            })
            .ok();
        Self { tx, join }
    }

    pub fn update(&self, data: OverlayData) {
        let _ = self.tx.send(Msg::Data(data));
    }

    pub fn stop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for LayerOverlay {
    fn drop(&mut self) {
        self.stop();
    }
}

const MIN_W: u32 = 240;
const MIN_H: u32 = 80;

fn run(rx: Channel<Msg>) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("sto-cla-overlay"), None);
    layer.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer.set_size(MIN_W * 2, MIN_H * 2);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.commit();

    let mut app = State {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        layer,
        conn: conn.clone(),
        width: MIN_W * 2,
        height: MIN_H * 2,
        gpu: None,
        data: OverlayData::default(),
        needs_redraw: true,
        stop: false,
    };

    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(handle.clone())?;
    handle
        .insert_source(rx, |event, _, app: &mut State| match event {
            ChannelEvent::Msg(Msg::Data(data)) => {
                app.data = data;
                app.needs_redraw = true;
            }
            ChannelEvent::Msg(Msg::Stop) => app.stop = true,
            ChannelEvent::Closed => app.stop = true,
        })
        .map_err(|e| format!("insert channel source: {e}"))?;

    while !app.stop {
        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut app)?;
        if app.needs_redraw {
            app.render();
            app.needs_redraw = false;
        }
    }
    Ok(())
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    egui_ctx: egui::Context,
    egui_renderer: egui_wgpu::Renderer,
}

struct State {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    layer: LayerSurface,
    conn: Connection,
    width: u32,
    height: u32,
    gpu: Option<Gpu>,
    data: OverlayData,
    needs_redraw: bool,
    stop: bool,
}

impl State {
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
                .expect("create_surface")
        };
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("no adapter");
        let (device, queue) = pollster::block_on(
            adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("cla-overlay"),
                ..Default::default()
            }),
        )
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

    #[allow(deprecated)]
    fn render(&mut self) {
        if self.gpu.is_none() {
            self.init_gpu();
        }
        let (w, h) = (self.width.max(1), self.height.max(1));
        let data = self.data.clone();
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
                egui::Grid::new("overlay").striped(true).show(ui, |ui| {
                    ui.label("Player");
                    for c in &data.columns {
                        ui.label(c);
                    }
                    ui.end_row();
                    for row in &data.rows {
                        ui.label(&row.name);
                        for v in &row.values {
                            ui.label(v);
                        }
                        ui.end_row();
                    }
                });
            });
        });

        // Auto-size the layer surface to the content on the next commit.
        let used = gpu.egui_ctx.used_size();
        let desired = (
            (used.x.ceil() as u32).max(MIN_W),
            (used.y.ceil() as u32).max(MIN_H),
        );
        if desired != (self.width, self.height) {
            self.width = desired.0;
            self.height = desired.1;
            self.layer.set_size(self.width, self.height);
            self.layer.commit();
            self.needs_redraw = true;
        }

        let clipped = gpu.egui_ctx.tessellate(full.shapes, ppp);
        for (id, delta) in &full.textures_delta.set {
            gpu.egui_renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        let frame = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => {
                gpu.surface.configure(&gpu.device, &gpu.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [w, h],
            pixels_per_point: ppp,
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let user_buffers =
            gpu.egui_renderer
                .update_buffers(&gpu.device, &gpu.queue, &mut encoder, &clipped, &screen);
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
        gpu.queue
            .submit(user_buffers.into_iter().chain(std::iter::once(encoder.finish())));
        frame.present();
        for id in &full.textures_delta.free {
            gpu.egui_renderer.free_texture(id);
        }
    }
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.stop = true;
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
        self.needs_redraw = true;
    }
}

impl CompositorHandler for State {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_shm!(State);
delegate_layer!(State);
delegate_registry!(State);
