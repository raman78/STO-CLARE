//! PoC: run egui on SDL3 + egui-wgpu, with NO winit / eframe.
//!
//! Goal: measure how much "platform glue" a winit-free stack needs for CLA.
//! Exercises the CLA-critical bits: mouse, text input, and file drag-and-drop
//! (CLA drops `.log` files). DPI/clipboard are marked as follow-ups.
//!
//! Run: `cargo run` inside `poc-sdl3/` (needs system libSDL3).

use egui_wgpu::ScreenDescriptor;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let window = video
        .window("CLA on SDL3 (no winit) — PoC", 900, 600)
        .position_centered()
        .resizable()
        .metal_view() // needed on macOS; harmless elsewhere
        .build()?;

    // --- wgpu init (wgpu 29 API), surface from the SDL3 window via raw-window-handle ---
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let surface = create_surface::create_surface(&instance, &window)?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
    }))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("device"),
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
    let mut renderer =
        egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());

    // TODO(dpi): wire SDL_GetWindowDisplayScale; 1.0 for the first pass.
    let pixels_per_point = 1.0_f32;

    // PoC state
    let mut click_count = 0u32;
    let mut text = String::from("type here — proves text input + focus");
    let mut last_dropped: Option<String> = None;

    // Per-frame input accumulation
    let mut events: Vec<egui::Event> = Vec::new();
    let mut modifiers = egui::Modifiers::default();
    let mut pointer_pos = egui::Pos2::ZERO;
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
                    width = w.max(1) as u32;
                    height = h.max(1) as u32;
                    config.width = width;
                    config.height = height;
                    surface.configure(&device, &config);
                }

                Event::MouseMotion { x, y, .. } => {
                    pointer_pos = egui::pos2(x / pixels_per_point, y / pixels_per_point);
                    events.push(egui::Event::PointerMoved(pointer_pos));
                }
                Event::MouseButtonDown { mouse_btn, .. } | Event::MouseButtonUp { mouse_btn, .. } => {
                    let pressed = matches!(event, Event::MouseButtonDown { .. });
                    if let Some(button) = to_egui_button(mouse_btn) {
                        events.push(egui::Event::PointerButton {
                            pos: pointer_pos,
                            button,
                            pressed,
                            modifiers,
                        });
                    }
                }
                Event::MouseWheel { x, y, .. } => {
                    events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: egui::vec2(x, y),
                        modifiers,
                        phase: egui::TouchPhase::Move,
                    });
                }

                Event::TextInput { text: t, .. } => {
                    events.push(egui::Event::Text(t));
                }
                Event::KeyDown { keycode, keymod, .. } | Event::KeyUp { keycode, keymod, .. } => {
                    let pressed = matches!(event, Event::KeyDown { .. });
                    modifiers = to_egui_modifiers(keymod);
                    if let Some(key) = keycode.and_then(to_egui_key) {
                        events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers,
                        });
                    }
                }

                // The whole point for CLA: dropping a .log file onto the window.
                Event::DropFile { filename, .. } => {
                    last_dropped = Some(filename);
                }

                _ => {}
            }
        }

        // --- run egui ---
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

        let full_output = egui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("CLA on SDL3 + egui-wgpu — no winit");
                ui.separator();
                if ui.button(format!("clicked {click_count} times")).clicked() {
                    click_count += 1;
                }
                ui.add(egui::TextEdit::singleline(&mut text));
                ui.separator();
                match &last_dropped {
                    Some(path) => ui.colored_label(egui::Color32::LIGHT_GREEN, format!("dropped: {path}")),
                    None => ui.weak("drag a .log file onto this window to test drag-and-drop"),
                };
            });
        });

        // --- render ---
        let paint_jobs = egui_ctx.tessellate(full_output.shapes, pixels_per_point);
        let screen = ScreenDescriptor {
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
                                r: 0.05,
                                g: 0.05,
                                b: 0.07,
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

    Ok(())
}

fn to_egui_button(b: MouseButton) -> Option<egui::PointerButton> {
    Some(match b {
        MouseButton::Left => egui::PointerButton::Primary,
        MouseButton::Right => egui::PointerButton::Secondary,
        MouseButton::Middle => egui::PointerButton::Middle,
        _ => return None,
    })
}

fn to_egui_modifiers(m: sdl3::keyboard::Mod) -> egui::Modifiers {
    use sdl3::keyboard::Mod;
    egui::Modifiers {
        alt: m.intersects(Mod::LALTMOD | Mod::RALTMOD),
        ctrl: m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
        shift: m.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD),
        mac_cmd: false,
        command: m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD),
    }
}

fn to_egui_key(k: Keycode) -> Option<egui::Key> {
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
        Keycode::Space => Key::Space,
        _ => return None,
    })
}

mod create_surface {
    use sdl3::video::Window;
    use wgpu::rwh::{HasDisplayHandle, HasWindowHandle};

    // wgpu needs Send+Sync on the surface target; wrap the borrowed window.
    struct SyncWindow<'a>(&'a Window);
    unsafe impl<'a> Send for SyncWindow<'a> {}
    unsafe impl<'a> Sync for SyncWindow<'a> {}

    impl<'a> HasWindowHandle for SyncWindow<'a> {
        fn window_handle(&self) -> Result<wgpu::rwh::WindowHandle<'_>, wgpu::rwh::HandleError> {
            self.0.window_handle()
        }
    }
    impl<'a> HasDisplayHandle for SyncWindow<'a> {
        fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
            self.0.display_handle()
        }
    }

    pub fn create_surface<'a>(
        instance: &wgpu::Instance,
        window: &'a Window,
    ) -> Result<wgpu::Surface<'a>, String> {
        instance
            .create_surface(SyncWindow(window))
            .map_err(|e| e.to_string())
    }
}
