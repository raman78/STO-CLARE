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
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
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
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, Proxy, QueueHandle,
};

use crate::custom_widgets::table::Table;

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
    Move(bool),
    Style(egui::Visuals),
    Stop,
}

/// Handle to the overlay thread held by the main app.
pub struct LayerOverlay {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
}

impl LayerOverlay {
    /// The `wgpu::Instance` is created and owned by the app (not by this
    /// thread): it must outlive every overlay show/hide cycle. A per-thread
    /// instance would tear down the Vulkan library on overlay close and leave
    /// eframe's main-window renderer calling into unloaded functions (segfault
    /// in `wait_for_fence`). The app keeps a clone alive, so the Vulkan library
    /// stays loaded even after this thread drops its own clone.
    pub fn spawn(instance: wgpu::Instance) -> Self {
        let (tx, rx) = channel::<Msg>();
        let join = std::thread::Builder::new()
            .name("cla-layer-overlay".into())
            .spawn(move || {
                if let Err(e) = run(rx, instance) {
                    log::error!("layer overlay: {e}");
                }
            })
            .ok();
        Self { tx, join }
    }

    pub fn update(&self, data: OverlayData) {
        let _ = self.tx.send(Msg::Data(data));
    }

    /// Enable "move" mode: the surface catches pointer input so it can be
    /// dragged. When disabled, clicks pass through to the game underneath.
    pub fn set_move(&self, move_mode: bool) {
        let _ = self.tx.send(Msg::Move(move_mode));
    }

    /// Match the overlay's colors to the main window by pushing its egui theme.
    pub fn set_style(&self, visuals: egui::Visuals) {
        let _ = self.tx.send(Msg::Style(visuals));
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

/// Linux input event code for the left mouse button (`BTN_LEFT`).
const BTN_LEFT: u32 = 0x110;

fn run(rx: Channel<Msg>, instance: wgpu::Instance) -> Result<(), Box<dyn std::error::Error>> {
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
        seat_state: SeatState::new(&globals, &qh),
        shm,
        compositor,
        layer,
        conn: conn.clone(),
        instance,
        width: MIN_W * 2,
        height: MIN_H * 2,
        gpu: None,
        data: OverlayData::default(),
        needs_redraw: true,
        stop: false,
        pointer: None,
        move_mode: false,
        dragging: false,
        pointer_pos: (0.0, 0.0),
        grab: (0.0, 0.0),
        margin: (0, 0),
        visuals: None,
        style_dirty: false,
    };
    // Start passive: clicks pass through to the game until "move" mode is on.
    app.apply_input_region();

    let mut event_loop: EventLoop<State> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(handle.clone())?;
    handle
        .insert_source(rx, |event, _, app: &mut State| match event {
            ChannelEvent::Msg(Msg::Data(data)) => {
                app.data = data;
                app.needs_redraw = true;
            }
            ChannelEvent::Msg(Msg::Move(move_mode)) => {
                if move_mode != app.move_mode {
                    app.move_mode = move_mode;
                    app.dragging = false;
                    app.apply_input_region();
                }
            }
            ChannelEvent::Msg(Msg::Style(visuals)) => {
                if app.visuals.is_none() {
                    app.needs_redraw = true; // first theme: draw with it right away
                }
                app.visuals = Some(visuals);
                app.style_dirty = true;
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
    // Tear down the wgpu surface (and device) before the wl_surface /
    // connection it was built from are dropped, to avoid a use-after-free.
    app.gpu = None;
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
    // `gpu` holds a wgpu surface built from the raw `conn`/`layer` handles, so
    // it MUST be dropped before them — struct fields drop in declaration order,
    // so keep it first. Dropping the wl_surface first would leave wgpu tearing
    // down a surface backed by a destroyed object (segfault on teardown).
    gpu: Option<Gpu>,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    shm: Shm,
    compositor: CompositorState,
    layer: LayerSurface,
    conn: Connection,
    // App-owned wgpu instance shared across overlay show/hide cycles; see
    // LayerOverlay::spawn for why it must not be created per-thread.
    instance: wgpu::Instance,
    width: u32,
    height: u32,
    data: OverlayData,
    needs_redraw: bool,
    stop: bool,
    // "Move" mode: when on, the surface catches pointer input and a left-button
    // drag repositions it; when off, an empty input region lets clicks fall
    // through to the game. `margin` is the (top, left) offset from the TOP|LEFT
    // anchor; `grab` is the surface-local point grabbed at drag start.
    pointer: Option<wl_pointer::WlPointer>,
    move_mode: bool,
    dragging: bool,
    pointer_pos: (f64, f64),
    grab: (f64, f64),
    margin: (i32, i32),
    // Theme pushed from the main app to match the main window; applied to the
    // egui context on the next render when `style_dirty`.
    visuals: Option<egui::Visuals>,
    style_dirty: bool,
}

impl State {
    /// Match the surface's input region to `move_mode`: the whole surface while
    /// moving (so it receives the drag), an empty region otherwise (clicks fall
    /// through to the game underneath).
    fn apply_input_region(&self) {
        let surface = self.layer.wl_surface();
        if self.move_mode {
            surface.set_input_region(None);
        } else if let Ok(region) = Region::new(&self.compositor) {
            surface.set_input_region(Some(region.wl_region()));
        }
        surface.commit();
    }

    /// Reposition the surface so the grabbed point follows the pointer. The
    /// margins re-reference against the moved surface, so `grab` stays constant.
    fn drag_to(&mut self, pos: (f64, f64)) {
        let dl = (pos.0 - self.grab.0).round() as i32;
        let dt = (pos.1 - self.grab.1).round() as i32;
        self.margin.1 = (self.margin.1 + dl).max(0);
        self.margin.0 = (self.margin.0 + dt).max(0);
        self.layer.set_margin(self.margin.0, 0, 0, self.margin.1);
        self.layer.commit();
    }

    fn init_gpu(&mut self) {
        let instance = &self.instance;

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
        // Adopt the main window's theme (colors, fills) pushed via set_style.
        let new_visuals = self.style_dirty.then(|| self.visuals.clone()).flatten();
        self.style_dirty = false;
        let gpu = self.gpu.as_mut().unwrap();
        if let Some(visuals) = new_visuals {
            gpu.egui_ctx.set_visuals(visuals);
        }
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
        // Rendered with the same custom `Table` as the desktop overlay so both
        // paths look identical; the table's own size drives the surface size.
        let mut required = egui::Vec2::ZERO;
        let full = gpu.egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let table_rect = Table::new(ui)
                    .min_scroll_height(f32::MAX)
                    .header(15.0, |h| {
                        h.cell(|ui| {
                            ui.label("Player");
                        });
                        for c in &data.columns {
                            h.cell(|ui| {
                                ui.label(c);
                            });
                        }
                    })
                    .body(25.0, |t| {
                        for row in &data.rows {
                            t.row(|r| {
                                r.cell(|ui| {
                                    ui.label(&row.name);
                                });
                                for v in &row.values {
                                    r.cell_with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(v);
                                        },
                                    );
                                }
                            });
                        }
                    });
                required = table_rect.size()
                    + ui.spacing().window_margin.left_top()
                    + ui.spacing().window_margin.right_bottom()
                    + ui.spacing().item_spacing;
            });
        });

        // The table measures column widths over a couple of frames; keep
        // redrawing until they settle.
        if gpu.egui_ctx.has_requested_repaint() {
            self.needs_redraw = true;
        }

        // Auto-size the layer surface to the content on the next commit.
        let desired = (
            (required.x.ceil() as u32).max(MIN_W),
            (required.y.ceil() as u32).max(MIN_H),
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

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            match event.kind {
                PointerEventKind::Enter { .. } => self.pointer_pos = event.position,
                PointerEventKind::Motion { .. } => {
                    self.pointer_pos = event.position;
                    if self.dragging {
                        self.drag_to(event.position);
                    }
                }
                PointerEventKind::Press { button, .. } if self.move_mode && button == BTN_LEFT => {
                    self.dragging = true;
                    self.grab = self.pointer_pos;
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    self.dragging = false;
                }
                _ => {}
            }
        }
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(State);
delegate_output!(State);
delegate_seat!(State);
delegate_pointer!(State);
delegate_shm!(State);
delegate_layer!(State);
delegate_registry!(State);

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises spawn -> render -> stop (the teardown path that segfaulted on
    /// overlay close). Manual: needs a Wayland session and briefly shows the
    /// overlay. Run with: `cargo test spawn_render_stop -- --ignored`.
    #[test]
    #[ignore = "requires a Wayland session"]
    fn spawn_render_stop() {
        let mut overlay = LayerOverlay::spawn(wgpu::Instance::default());
        overlay.update(OverlayData {
            columns: vec!["DPS".into()],
            rows: vec![OverlayRow {
                name: "Test".into(),
                values: vec!["123.4k".into()],
            }],
        });
        // Toggle "move" mode both ways to exercise the input-region path.
        overlay.set_move(true);
        std::thread::sleep(std::time::Duration::from_millis(400));
        overlay.set_move(false);
        std::thread::sleep(std::time::Duration::from_millis(400));
        overlay.stop(); // must return without crashing the process
    }

    /// Reproduces toggling the overlay off and on again (drop -> re-spawn),
    /// which crashed the app when it had been shown during a game.
    #[test]
    #[ignore = "requires a Wayland session"]
    fn spawn_stop_respawn() {
        // A single app-owned instance kept alive across every cycle, matching
        // how the app shares it (see LayerOverlay::spawn).
        let instance = wgpu::Instance::default();
        for _ in 0..3 {
            let mut overlay = LayerOverlay::spawn(instance.clone());
            overlay.update(OverlayData {
                columns: vec!["DPS".into()],
                rows: vec![OverlayRow {
                    name: "Test".into(),
                    values: vec!["123.4k".into()],
                }],
            });
            std::thread::sleep(std::time::Duration::from_millis(400));
            overlay.stop();
        }
    }
}
