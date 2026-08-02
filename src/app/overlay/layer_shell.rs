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
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use egui_wgpu::wgpu;
use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::reexports::calloop::{
    EventLoop,
    channel::{Channel, Event as ChannelEvent, Sender, channel},
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
};
use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::client::{
    zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
    zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
    shm::{Shm, ShmHandler},
};

use crossbeam_channel::{Receiver as EventRx, Sender as EventTx, unbounded};

use crate::custom_widgets::table::Table;

/// Snapshot of what the overlay should display. Plain, `Send` data.
#[derive(Clone, Default)]
pub struct OverlayData {
    pub columns: Vec<String>,
    pub rows: Vec<OverlayRow>,
    /// Every configurable column with its enabled flag, for the ⛭ popup on the
    /// overlay. Order matches the app's column list so `ToggleColumn(i)` maps
    /// straight back. See [`OverlayEvent`].
    pub all_columns: Vec<(String, bool)>,
}

#[derive(Clone)]
pub struct OverlayRow {
    pub name: String,
    pub values: Vec<String>,
}

/// Sent from the overlay thread back to the app (the overlay owns its own
/// toolbar now). The app applies these and recomputes the displayed data.
pub enum OverlayEvent {
    ToggleColumn(usize),
}

enum Msg {
    Data(OverlayData),
    Style(Arc<egui::Style>),
    Stop,
}

/// Handle to the overlay thread held by the main app.
pub struct LayerOverlay {
    tx: Sender<Msg>,
    join: Option<JoinHandle<()>>,
    /// Current (top, left) anchor margin, updated by the thread as the user
    /// drags, so the app can persist the position. See [`LayerOverlay::spawn`].
    position: Arc<Mutex<(i32, i32)>>,
    /// Events raised by the overlay's own toolbar (e.g. column toggles).
    events: EventRx<OverlayEvent>,
}

/// The wgpu handles shared by eframe's main window and the layer-shell overlay,
/// so the app runs a single Vulkan stack instead of two. Created once at startup
/// (see [`create_shared_gpu`]) and handed to eframe via `WgpuSetup::Existing`.
/// All fields are cheap `Arc`-backed clones and are `Send + Sync`, so they cross
/// into the overlay thread freely.
#[derive(Clone)]
pub struct OverlayGpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Creates the one wgpu instance/adapter/device/queue the whole app uses. The
/// handles go to eframe (`WgpuSetup::Existing`) for the main window and to the
/// layer-shell overlay via [`OverlayGpu`], so both render through the same
/// device — no second instance, and no Vulkan-teardown races on overlay close.
pub fn create_shared_gpu() -> OverlayGpu {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("no wgpu adapter for the shared overlay/main-window device");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("sto-clare-shared"),
        // Mirror egui-wgpu's default device limits so eframe accepts the device
        // we hand it (it wants a 4k+ capable max_texture_dimension_2d).
        required_limits: wgpu::Limits {
            max_texture_dimension_2d: 8192,
            ..wgpu::Limits::default()
        },
        ..Default::default()
    }))
    .expect("no wgpu device for the shared overlay/main-window device");
    OverlayGpu {
        instance,
        adapter,
        device,
        queue,
    }
}

impl LayerOverlay {
    /// The [`OverlayGpu`] handles are created once by the app and shared with
    /// eframe's main-window renderer, so the overlay never spins up a second
    /// Vulkan stack. They outlive every show/hide cycle; this thread only holds
    /// clones, so closing the overlay tears down just its surface, never the
    /// shared device.
    pub fn spawn(gpu: OverlayGpu, initial_position: (i32, i32)) -> Self {
        let (tx, rx) = channel::<Msg>();
        let (events_tx, events) = unbounded::<OverlayEvent>();
        let position = Arc::new(Mutex::new(initial_position));
        let thread_position = Arc::clone(&position);
        let join = std::thread::Builder::new()
            .name("cla-layer-overlay".into())
            .spawn(move || {
                if let Err(e) = run(rx, gpu, thread_position, events_tx) {
                    log::error!("layer overlay: {e}");
                }
            })
            .ok();
        Self {
            tx,
            join,
            position,
            events,
        }
    }

    /// Current (top, left) anchor margin, for persisting the overlay position.
    pub fn position(&self) -> (i32, i32) {
        *self.position.lock().unwrap()
    }

    /// Drains events raised by the overlay's toolbar since the last call.
    pub fn poll_events(&self) -> Vec<OverlayEvent> {
        self.events.try_iter().collect()
    }

    pub fn update(&self, data: OverlayData) {
        let _ = self.tx.send(Msg::Data(data));
    }

    /// Match the overlay to the main window by pushing its full egui style
    /// (colors, spacing, text styles), so both look identical.
    pub fn set_style(&self, style: Arc<egui::Style>) {
        let _ = self.tx.send(Msg::Style(style));
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

fn run(
    rx: Channel<Msg>,
    gpu: OverlayGpu,
    position: Arc<Mutex<(i32, i32)>>,
    events_tx: EventTx<OverlayEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    let OverlayGpu {
        instance,
        adapter,
        device,
        queue,
    } = gpu;
    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)?;
    let shm = Shm::bind(&globals, &qh)?;

    // Restore the saved (top, left) offset from the TOP|LEFT anchor.
    let margin = *position.lock().unwrap();
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(
        &qh,
        surface,
        Layer::Overlay,
        Some("sto-clare-overlay"),
        None,
    );
    layer.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer.set_margin(margin.0, 0, 0, margin.1);
    layer.set_size(MIN_W * 2, MIN_H * 2);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    // Ignore other surfaces' exclusive zones and reserve none of our own, so
    // dragging only moves us (the exclusive zone includes the margin, and an
    // auto/default zone makes the compositor reflow on every move -> jitter).
    layer.set_exclusive_zone(-1);
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
        adapter,
        device,
        queue,
        width: MIN_W * 2,
        height: MIN_H * 2,
        gpu: None,
        data: OverlayData::default(),
        needs_redraw: true,
        stop: false,
        pointer: None,
        rel_manager: globals
            .bind::<ZwpRelativePointerManagerV1, _, _>(&qh, 1..=1, ())
            .ok(),
        rel_pointer: None,
        move_mode: false,
        dragging: false,
        pointer_pos: (0.0, 0.0),
        drag_delta: (0.0, 0.0),
        margin,
        output_size: None,
        position,
        style: None,
        style_dirty: false,
        events_tx,
        settings_open: false,
        egui_events: Vec::new(),
        pointer_on_toolbar: false,
        toolbar_rect: (0, 0, 0, 0),
    };
    // Start passive: only the toolbar strip catches clicks; the rest passes
    // through to the game until "move" mode or the settings popup is on.
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
            ChannelEvent::Msg(Msg::Style(style)) => {
                if app.style.is_none() {
                    app.needs_redraw = true; // first theme: draw with it right away
                }
                app.style = Some(style);
                app.style_dirty = true;
            }
            ChannelEvent::Msg(Msg::Stop) => app.stop = true,
            ChannelEvent::Closed => app.stop = true,
        })
        .map_err(|e| format!("insert channel source: {e}"))?;

    while !app.stop {
        event_loop.dispatch(Some(std::time::Duration::from_millis(16)), &mut app)?;
        if app.needs_redraw {
            // Reset before rendering: render() may set it again (toolbar toggle,
            // table settling) to request another frame right away. Resetting
            // after would clobber that and defer the redraw to the next event.
            app.needs_redraw = false;
            app.render();
        }
    }
    // Drop the overlay's wgpu surface before the wl_surface / connection it was
    // built from, to avoid a use-after-free. The device/queue are shared clones,
    // so this tears down only the surface, never the shared device.
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
    // wgpu handles shared with eframe's main-window renderer (see
    // create_shared_gpu). `instance` builds the layer-shell surface; the
    // adapter/device/queue are the very ones eframe uses, so there is a single
    // Vulkan stack. These are clones, so closing the overlay drops only its
    // surface, never the shared device.
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
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
    // Relative-pointer motion (independent of the surface position) drives the
    // drag, so moving the surface can't feed back into the reported coordinates
    // and make it jitter. `drag_delta` accumulates sub-pixel motion between
    // frames; the whole-pixel part is applied to the margin each render.
    rel_manager: Option<ZwpRelativePointerManagerV1>,
    rel_pointer: Option<ZwpRelativePointerV1>,
    move_mode: bool,
    dragging: bool,
    pointer_pos: (f64, f64),
    drag_delta: (f64, f64),
    margin: (i32, i32),
    // Logical size of the output the overlay is on, to keep it on-screen.
    output_size: Option<(i32, i32)>,
    // Shared with the app handle so it can persist the dragged position.
    position: Arc<Mutex<(i32, i32)>>,
    // Full egui style pushed from the main app to match the main window;
    // applied to the overlay's egui context on the next render when `style_dirty`.
    style: Option<Arc<egui::Style>>,
    style_dirty: bool,
    // Toolbar (⛭/✋) lives on the overlay itself. `events_tx` reports column
    // toggles back to the app; `settings_open` is the ⛭ popup state (also
    // widens the input region); `egui_events` are pointer events fed into egui
    // so its buttons work; `pointer_on_toolbar` gates dragging vs clicking.
    events_tx: EventTx<OverlayEvent>,
    settings_open: bool,
    egui_events: Vec<egui::Event>,
    pointer_on_toolbar: bool,
    // The icon toolbar's rect in surface pixels, measured each render, so the
    // input region and the drag/click hit-test match the icons exactly.
    toolbar_rect: (i32, i32, i32, i32),
}

/// Height (points, = pixels at scale 1) of the always-clickable toolbar strip
/// at the bottom of the overlay.
const TOOLBAR_H: u32 = 26;

impl State {
    /// Set the surface's input region for the current mode: the whole surface
    /// while moving or with the settings popup open (so drags and popup clicks
    /// land), otherwise just the top toolbar strip (its ⛭/✋ buttons stay
    /// clickable while the rest passes clicks through to the game).
    fn apply_input_region(&self) {
        let surface = self.layer.wl_surface();
        if self.move_mode || self.settings_open {
            surface.set_input_region(None);
        } else if let Ok(region) = Region::new(&self.compositor) {
            let (x, y, w, h) = self.toolbar_rect;
            if w > 0 && h > 0 {
                region.add(x, y, w, h);
            } else {
                // Before the first render: a bottom strip as a fallback.
                let top = self.height.saturating_sub(TOOLBAR_H) as i32;
                region.add(0, top, self.width.max(1) as i32, TOOLBAR_H as i32);
            }
            surface.set_input_region(Some(region.wl_region()));
        }
        surface.commit();
    }

    /// Applies the accumulated relative-pointer motion to the margin (once per
    /// frame). Uses raw pointer deltas, so moving the surface never changes the
    /// input and the drag stays jitter-free. The margin is only set pending; the
    /// frame's wgpu present commits it together with the buffer (atomic move).
    fn apply_drag(&mut self) {
        let (max_left, max_top) = self.max_margin();
        let ix = self.drag_delta.0.trunc() as i32;
        let iy = self.drag_delta.1.trunc() as i32;
        if ix == 0 && iy == 0 {
            return;
        }
        self.drag_delta.0 -= ix as f64;
        self.drag_delta.1 -= iy as f64;
        self.margin.1 = (self.margin.1 + ix).clamp(0, max_left);
        self.margin.0 = (self.margin.0 + iy).clamp(0, max_top);
        self.layer.set_margin(self.margin.0, 0, 0, self.margin.1);
        *self.position.lock().unwrap() = self.margin;
    }

    /// Largest (left, top) margin that keeps the overlay on its output; falls
    /// back to unrestricted when the output size isn't known yet.
    fn max_margin(&self) -> (i32, i32) {
        match self.output_size {
            Some((ow, oh)) => (
                (ow - self.width as i32).max(0),
                (oh - self.height as i32).max(0),
            ),
            None => (i32::MAX, i32::MAX),
        }
    }

    /// Clamps the margin onto the output; returns whether it changed.
    fn clamp_margin(&mut self) -> bool {
        let (max_left, max_top) = self.max_margin();
        let clamped = (
            self.margin.0.clamp(0, max_top),
            self.margin.1.clamp(0, max_left),
        );
        if clamped != self.margin {
            self.margin = clamped;
            true
        } else {
            false
        }
    }

    fn commit_margin(&mut self) {
        self.layer.set_margin(self.margin.0, 0, 0, self.margin.1);
        self.layer.commit();
        *self.position.lock().unwrap() = self.margin;
    }

    fn init_gpu(&mut self) {
        let display_ptr =
            NonNull::new(self.conn.backend().display_ptr() as *mut _).expect("null wl_display");
        let surface_ptr =
            NonNull::new(self.layer.wl_surface().id().as_ptr() as *mut _).expect("null wl_surface");
        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(display_ptr));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(surface_ptr));

        // Build the surface from the shared instance and reuse the shared
        // adapter/device/queue — no second adapter/device request here.
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display),
                    raw_window_handle: raw_window,
                })
                .expect("create_surface")
        };

        let caps = surface.get_capabilities(&self.adapter);
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
        surface.configure(&self.device, &config);
        let egui_renderer =
            egui_wgpu::Renderer::new(&self.device, format, egui_wgpu::RendererOptions::default());

        self.gpu = Some(Gpu {
            device: self.device.clone(),
            queue: self.queue.clone(),
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
        // Apply the frame's accumulated relative-pointer motion to the position.
        if self.dragging {
            self.apply_drag();
        }
        let (w, h) = (self.width.max(1), self.height.max(1));
        let data = self.data.clone();
        let move_mode = self.move_mode;
        let settings_open = self.settings_open;
        let input_events = std::mem::take(&mut self.egui_events);
        // Adopt the main window's full style (colors, spacing, text) pushed via
        // set_style, so the overlay matches the main window.
        let new_style = self.style_dirty.then(|| self.style.clone()).flatten();
        self.style_dirty = false;
        let gpu = self.gpu.as_mut().unwrap();
        if let Some(style) = new_style {
            gpu.egui_ctx.set_global_style(style);
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
            events: input_events,
            ..Default::default()
        };
        // Rendered with the same custom `Table` as the desktop overlay so both
        // paths look identical. The overlay owns its toolbar (⛭ column config,
        // ✋ move); clicks are collected here and applied after the frame.
        let mut required = egui::Vec2::ZERO;
        let mut toggle_move = false;
        let mut toggle_settings = false;
        let mut toggled_columns: Vec<usize> = Vec::new();
        let mut toolbar_rect = egui::Rect::NOTHING;
        // apply_input_region borrows all of `self`; `gpu` is borrowed until the
        // end of render, so defer it behind this flag.
        let mut region_dirty = false;
        let full = gpu.egui_ctx.run_ui(raw_input, |ui| {
            let style = ui.ctx().global_style();
            let frame = egui::Frame::central_panel(&style)
                .stroke(style.visuals.window_stroke)
                .inner_margin(4.0);
            egui::CentralPanel::default()
                .frame(frame)
                .show_inside(ui, |ui| {
                    // The refreshing part: the DPS table. Its measured rect drives
                    // the surface size (content-sized, unlike ui.min_rect() which
                    // includes fill-width widgets and makes the surface oscillate).
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

                    // Column popup (when open) and the icon toolbar at the bottom,
                    // like the live parser. Measure their height via the cursor so
                    // the surface can size to table + toolbar without fill-width
                    // widgets feeding back into the width.
                    let bottom_start = ui.cursor().top();
                    if settings_open {
                        for (index, (name, enabled)) in data.all_columns.iter().enumerate() {
                            let mut enabled = *enabled;
                            if ui.checkbox(&mut enabled, name).clicked() {
                                toggled_columns.push(index);
                            }
                        }
                    }
                    toolbar_rect = ui
                        .horizontal(|ui| {
                            // Square, fully-clickable icon buttons (the plain label
                            // only reacts on the glyph itself).
                            let icon = egui::vec2(TOOLBAR_H as f32 - 6.0, TOOLBAR_H as f32 - 6.0);
                            if ui
                                .add_sized(icon, egui::Button::selectable(settings_open, "⛭"))
                                .clicked()
                            {
                                toggle_settings = true;
                            }
                            if ui
                                .add_sized(icon, egui::Button::selectable(move_mode, "✋"))
                                .clicked()
                            {
                                toggle_move = true;
                            }
                        })
                        .response
                        .rect;
                    let bottom_height = ui.cursor().top() - bottom_start;

                    required = egui::vec2(table_rect.width(), table_rect.height() + bottom_height)
                        + ui.spacing().window_margin.left_top()
                        + ui.spacing().window_margin.right_bottom();
                });
        });

        // Remember the toolbar's rect (points == pixels at scale 1), padded a
        // little, so the input region and hit-test cover the icons exactly.
        if toolbar_rect.is_finite() && toolbar_rect.area() > 0.0 {
            let pad = 2.0;
            self.toolbar_rect = (
                (toolbar_rect.min.x - pad).floor().max(0.0) as i32,
                (toolbar_rect.min.y - pad).floor().max(0.0) as i32,
                (toolbar_rect.width() + 2.0 * pad).ceil() as i32,
                (toolbar_rect.height() + 2.0 * pad).ceil() as i32,
            );
        }

        // Apply the toolbar interactions collected during the frame.
        for index in toggled_columns {
            let _ = self.events_tx.send(OverlayEvent::ToggleColumn(index));
            // Flip the checkbox locally so it reacts instantly; the app's
            // authoritative update (which also refreshes the table columns)
            // arrives within a refresh and matches.
            if let Some(column) = self.data.all_columns.get_mut(index) {
                column.1 = !column.1;
            }
            self.needs_redraw = true;
        }
        if toggle_settings {
            self.settings_open = !self.settings_open;
        }
        if toggle_move {
            self.move_mode = !self.move_mode;
            self.dragging = false;
        }
        if toggle_settings || toggle_move {
            region_dirty = true;
            // Redraw now so the new state shows immediately instead of waiting
            // for the next ~500 ms data update (perceived as a click lag).
            self.needs_redraw = true;
        }

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
            // Growing (e.g. opening the settings popup) near an edge would push
            // the overlay off-screen; pull it back so it stays fully visible.
            // Inlined (clamp_margin borrows all of self while gpu is borrowed).
            if let Some((ow, oh)) = self.output_size {
                self.margin.1 = self.margin.1.clamp(0, (ow - self.width as i32).max(0));
                self.margin.0 = self.margin.0.clamp(0, (oh - self.height as i32).max(0));
            }
            self.layer.set_margin(self.margin.0, 0, 0, self.margin.1);
            self.layer.set_size(self.width, self.height);
            self.layer.commit();
            *self.position.lock().unwrap() = self.margin;
            // The toolbar input strip spans the surface width; refresh it.
            region_dirty = true;
            self.needs_redraw = true;
        }

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

        // gpu is no longer borrowed here, so the whole-`self` input-region
        // update (mode/size changed) can run.
        if region_dirty {
            self.apply_input_region();
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

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.learn_output(&output);
    }
    fn update_output(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.learn_output(&output);
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl State {
    /// Records the output's size and pulls the overlay back on-screen if a
    /// stale (e.g. restored) margin left it off the edge.
    fn learn_output(&mut self, output: &wl_output::WlOutput) {
        if let Some(size) = self.output_state.info(output).and_then(|i| i.logical_size) {
            self.output_size = Some(size);
            if self.clamp_margin() {
                self.commit_margin();
            }
        }
    }
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
            let pointer = self.seat_state.get_pointer(qh, &seat).ok();
            // Attach a relative pointer to the same wl_pointer for jitter-free
            // dragging (raw motion deltas, independent of the surface position).
            if let (Some(pointer), Some(manager)) = (&pointer, &self.rel_manager) {
                self.rel_pointer = Some(manager.get_relative_pointer(pointer, qh, ()));
            }
            self.pointer = pointer;
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
            let pos = event.position;
            let (tx, ty, tw, th) = self.toolbar_rect;
            let on_toolbar = tw > 0
                && th > 0
                && pos.0 >= tx as f64
                && pos.0 < (tx + tw) as f64
                && pos.1 >= ty as f64
                && pos.1 < (ty + th) as f64;
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    self.pointer_pos = pos;
                    self.pointer_on_toolbar = on_toolbar;
                    self.egui_events
                        .push(egui::Event::PointerMoved(egui_pos(pos)));
                    // Actual dragging happens once per rendered frame (see
                    // render); doing it per motion event double-counts the
                    // async coordinate shift and flings the surface around.
                }
                PointerEventKind::Leave { .. } => {
                    self.egui_events.push(egui::Event::PointerGone);
                }
                PointerEventKind::Press { button, .. } if button == BTN_LEFT => {
                    // In move mode a press on empty space starts a drag; presses
                    // on the toolbar, or anywhere while the settings popup is
                    // open, go to egui so its buttons/checkboxes work.
                    if self.move_mode && !on_toolbar && !self.settings_open {
                        self.dragging = true;
                        self.drag_delta = (0.0, 0.0);
                    } else {
                        self.egui_events.push(pointer_button(pos, true));
                    }
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    if self.dragging {
                        self.dragging = false;
                    } else {
                        self.egui_events.push(pointer_button(pos, false));
                    }
                }
                _ => {}
            }
        }
        self.needs_redraw = true;
    }
}

fn egui_pos(pos: (f64, f64)) -> egui::Pos2 {
    egui::pos2(pos.0 as f32, pos.1 as f32)
}

fn pointer_button(pos: (f64, f64), pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: egui_pos(pos),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

// The relative-pointer protocol isn't covered by an SCTK delegate, so dispatch
// it by hand. The manager has no events; the pointer reports raw motion.
impl Dispatch<ZwpRelativePointerManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpRelativePointerManagerV1,
        _: <ZwpRelativePointerManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpRelativePointerV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event {
            if state.dragging {
                state.drag_delta.0 += dx;
                state.drag_delta.1 += dy;
                state.needs_redraw = true;
            }
        }
    }
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
        let mut overlay = LayerOverlay::spawn(create_shared_gpu(), (100, 50));
        // The saved position is exposed back for persistence.
        assert_eq!(overlay.position(), (100, 50));
        overlay.update(OverlayData {
            columns: vec!["DPS".into()],
            rows: vec![OverlayRow {
                name: "Test".into(),
                values: vec!["123.4k".into()],
            }],
            all_columns: vec![("DPS".into(), true), ("Deaths".into(), false)],
        });
        std::thread::sleep(std::time::Duration::from_millis(800));
        overlay.stop(); // must return without crashing the process
    }

    /// Reproduces toggling the overlay off and on again (drop -> re-spawn),
    /// which crashed the app when it had been shown during a game.
    #[test]
    #[ignore = "requires a Wayland session"]
    fn spawn_stop_respawn() {
        // A single app-owned gpu kept alive across every cycle, matching how the
        // app shares it (see LayerOverlay::spawn).
        let gpu = create_shared_gpu();
        for _ in 0..3 {
            let mut overlay = LayerOverlay::spawn(gpu.clone(), (0, 0));
            overlay.update(OverlayData {
                columns: vec!["DPS".into()],
                rows: vec![OverlayRow {
                    name: "Test".into(),
                    values: vec!["123.4k".into()],
                }],
                ..Default::default()
            });
            std::thread::sleep(std::time::Duration::from_millis(400));
            overlay.stop();
        }
    }
}
