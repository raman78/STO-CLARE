//! Windowing / platform layer on SDL3 (replaces eframe/winit).
//!
//! Runs the egui `App` inside a hand-rolled SDL3 + egui-wgpu loop. The Linux
//! overlay stays a separate `wlr-layer-shell` surface (see app::layer_overlay);
//! this only drives the main window.
//!
//! First-pass follow-ups (marked TODO): HiDPI (`pixels_per_point`), clipboard
//! copy/paste, cursor icons, window icon, maximized-state persistence.

use egui::CursorIcon;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;

use crate::app::App;

/// A stable handle to the main window, for parenting native file dialogs (rfd).
/// Holds owned raw handles copied from the SDL3 window (valid for its lifetime).
pub struct AppWindow {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

impl HasWindowHandle for AppWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: the SDL3 window outlives every dialog parented to it.
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}
impl HasDisplayHandle for AppWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;

    let (saved_size, _maximized) = crate::app::saved_window_geometry();
    let size = saved_size.unwrap_or(egui::vec2(1280.0, 720.0));
    let mut window = video
        .window(
            &format!("STO_CombatLogAnalyzer V{}", env!("CARGO_PKG_VERSION")),
            size.x as u32,
            size.y as u32,
        )
        .position_centered()
        .resizable()
        .high_pixel_density()
        .metal_view()
        .build()?;
    window.set_minimum_size(800, 600).ok();
    // SDL3 needs text input started explicitly, else typing produces nothing.
    video.text_input().start(&window);

    let app_window = AppWindow {
        window: window.window_handle()?.as_raw(),
        display: window.display_handle()?.as_raw(),
    };

    // --- wgpu ---
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let surface = create_surface(&instance, &window)?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("cla device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        trace: wgpu::Trace::Off,
    }))?;
    let caps = surface.get_capabilities(&adapter);
    let format = *caps
        .formats
        .iter()
        .find(|f| f.is_srgb())
        .unwrap_or(&caps.formats[0]);
    let (mut width, mut height) = window.size();
    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::Auto,
        desired_maximum_frame_latency: 2,
        view_formats: vec![],
    };
    surface.configure(&device, &config);

    // --- egui ---
    let egui_ctx = egui::Context::default();
    egui_ctx.memory_mut(|m| m.options.repaint_on_widget_change = false);
    let pixels_per_point = 1.0_f32; // TODO(dpi): SDL_GetWindowDisplayScale
    egui_ctx.set_pixels_per_point(pixels_per_point);
    let mut renderer = egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

    let mut app = App::new(&egui_ctx, pixels_per_point);

    let mut events: Vec<egui::Event> = Vec::new();
    let mut modifiers = egui::Modifiers::default();
    let mut pointer = egui::Pos2::ZERO;
    let start = std::time::Instant::now();

    let mut event_pump = sdl.event_pump()?;
    'running: loop {
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } => break 'running,
                Event::Window {
                    win_event: WindowEvent::PixelSizeChanged(w, h) | WindowEvent::Resized(w, h),
                    window_id,
                    ..
                } if window_id == window.id() => {
                    width = (w.max(1)) as u32;
                    height = (h.max(1)) as u32;
                    config.width = width;
                    config.height = height;
                    surface.configure(&device, &config);
                }
                Event::MouseMotion { x, y, .. } => {
                    pointer = egui::pos2(x / pixels_per_point, y / pixels_per_point);
                    events.push(egui::Event::PointerMoved(pointer));
                }
                Event::MouseButtonDown { mouse_btn, .. } | Event::MouseButtonUp { mouse_btn, .. } => {
                    let pressed = matches!(event, Event::MouseButtonDown { .. });
                    if let Some(button) = to_button(mouse_btn) {
                        events.push(egui::Event::PointerButton {
                            pos: pointer,
                            button,
                            pressed,
                            modifiers,
                        });
                    }
                }
                Event::MouseWheel { x, y, .. } => events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(x, y),
                    modifiers,
                    phase: egui::TouchPhase::Move,
                }),
                Event::TextInput { text, .. } => events.push(egui::Event::Text(text)),
                Event::KeyDown { keycode, keymod, .. } | Event::KeyUp { keycode, keymod, .. } => {
                    let pressed = matches!(event, Event::KeyDown { .. });
                    modifiers = to_modifiers(keymod);
                    if let Some(key) = keycode.and_then(to_key) {
                        events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers,
                        });
                    }
                }
                Event::DropFile { filename, .. } => {
                    events.push(egui::Event::Text(String::new())); // wake a frame
                    egui_ctx.input_mut(|i| {
                        i.raw.dropped_files.push(egui::DroppedFile {
                            path: Some(std::path::PathBuf::from(&filename)),
                            ..Default::default()
                        })
                    });
                }
                _ => {}
            }
        }

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32 / pixels_per_point, height as f32 / pixels_per_point),
            )),
            time: Some(start.elapsed().as_secs_f64()),
            modifiers,
            events: std::mem::take(&mut events),
            ..Default::default()
        };

        // Track window geometry (debounced save) using SDL's size.
        app.track_window_geometry(
            [width as f32 / pixels_per_point, height as f32 / pixels_per_point],
            false, // TODO(maximized)
            start.elapsed().as_secs_f64(),
        );

        let full_output = egui_ctx.run(raw_input, |ctx| app.update(ctx, &app_window));

        handle_platform_output(&full_output.platform_output);

        let paint_jobs = egui_ctx.tessellate(full_output.shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };
        for (id, delta) in &full_output.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, delta);
        }

        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => continue,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&device, &config);
                continue;
            }
            other => panic!("surface texture: {other:?}"),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui") });
        let user_bufs = renderer.update_buffers(&device, &queue, &mut encoder, &paint_jobs, &screen);
        {
            let mut rpass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.02,
                                g: 0.02,
                                b: 0.03,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            renderer.render(&mut rpass, &paint_jobs, &screen);
        }
        queue.submit(user_bufs.into_iter().chain(std::iter::once(encoder.finish())));
        frame.present();

        for id in &full_output.textures_delta.free {
            renderer.free_texture(id);
        }
    }

    app.save_on_exit();
    Ok(())
}

fn handle_platform_output(out: &egui::PlatformOutput) {
    for cmd in &out.commands {
        match cmd {
            // TODO(clipboard): wire SDL clipboard for CopyText.
            egui::OutputCommand::OpenUrl(open) => open_url(&open.url),
            _ => {}
        }
    }
    let _ = CursorIcon::Default; // TODO(cursor): map out.cursor_icon to SDL cursors
}

fn open_url(url: &str) {
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "windows")]
    let opener = "explorer";
    let _ = std::process::Command::new(opener).arg(url).spawn();
}

fn create_surface<'a>(
    instance: &wgpu::Instance,
    window: &'a sdl3::video::Window,
) -> Result<wgpu::Surface<'a>, String> {
    struct SyncWindow<'a>(&'a sdl3::video::Window);
    unsafe impl<'a> Send for SyncWindow<'a> {}
    unsafe impl<'a> Sync for SyncWindow<'a> {}
    impl<'a> HasWindowHandle for SyncWindow<'a> {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            self.0.window_handle()
        }
    }
    impl<'a> HasDisplayHandle for SyncWindow<'a> {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            self.0.display_handle()
        }
    }
    instance
        .create_surface(SyncWindow(window))
        .map_err(|e| e.to_string())
}

fn to_button(b: MouseButton) -> Option<egui::PointerButton> {
    Some(match b {
        MouseButton::Left => egui::PointerButton::Primary,
        MouseButton::Right => egui::PointerButton::Secondary,
        MouseButton::Middle => egui::PointerButton::Middle,
        _ => return None,
    })
}

fn to_modifiers(m: sdl3::keyboard::Mod) -> egui::Modifiers {
    use sdl3::keyboard::Mod;
    let ctrl = m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
    egui::Modifiers {
        alt: m.intersects(Mod::LALTMOD | Mod::RALTMOD),
        ctrl,
        shift: m.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
        mac_cmd: false,
        command: ctrl,
    }
}

fn to_key(k: Keycode) -> Option<egui::Key> {
    use egui::Key;
    Some(match k {
        Keycode::Backspace => Key::Backspace,
        Keycode::Return => Key::Enter,
        Keycode::Tab => Key::Tab,
        Keycode::Escape => Key::Escape,
        Keycode::Left => Key::ArrowLeft,
        Keycode::Right => Key::ArrowRight,
        Keycode::Up => Key::ArrowUp,
        Keycode::Down => Key::ArrowDown,
        Keycode::Home => Key::Home,
        Keycode::End => Key::End,
        Keycode::Delete => Key::Delete,
        Keycode::PageUp => Key::PageUp,
        Keycode::PageDown => Key::PageDown,
        Keycode::A => Key::A,
        Keycode::C => Key::C,
        Keycode::V => Key::V,
        Keycode::X => Key::X,
        Keycode::Z => Key::Z,
        _ => return None,
    })
}
