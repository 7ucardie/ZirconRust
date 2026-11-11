//! ZirconRust game client (winit + wgpu).
//!
//! ```text
//! mir-client [--server HOST:PORT] [--assets DIR] [--size WIDTHxHEIGHT]
//!   ZIRCON_ASSETS  client root containing Data/ and Map/ (default ~/zircon-assets/Client)
//!   ZIRCON_SERVER  server address (default 127.0.0.1:7000)
//! ```

mod anim;
mod assets;
mod audio;
mod character_services;
mod chat_options;
mod chat_panel;
mod client;
mod dungeon_codex;
mod effects;
mod equipment_fx;
mod fortune;
mod friends;
mod game;
mod gfx;
mod items;
mod market;
mod menu;
mod net;
mod overlay;
mod ranking;
mod scenes;
mod settings;
mod sound_table;
mod text;
mod ui;
mod wardrobe;
mod windows;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::assets::Assets;
use crate::client::Client;
use crate::gfx::{Gpu, SpriteRenderer};
use crate::text::TextLayer;

struct Args {
    assets: PathBuf,
    server: String,
    size: (u32, u32),
}

fn window_size(value: &str) -> Option<(u32, u32)> {
    let (width, height) = value.split_once('x')?;
    let (width, height) = (width.parse().ok()?, height.parse().ok()?);
    crate::settings::valid_resolution((width, height)).then_some((width, height))
}

fn parse_args() -> Args {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut assets = std::env::var_os("ZIRCON_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("zircon-assets/Client"));
    let mut server = std::env::var("ZIRCON_SERVER")
        .unwrap_or_else(|_| format!("127.0.0.1:{}", mir_proto::DEFAULT_PORT));
    let mut size = std::env::var("ZIRCON_WINDOW_SIZE")
        .ok()
        .and_then(|value| window_size(&value))
        .or_else(|| crate::settings::Preferences::load().resolution)
        .unwrap_or((1024, 768));
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--assets" => assets = PathBuf::from(it.next().expect("--assets DIR")),
            "--server" => server = it.next().expect("--server HOST:PORT"),
            "--size" => {
                let value = it.next().expect("--size WIDTHxHEIGHT");
                size = window_size(&value).expect("--size needs WIDTHxHEIGHT, at least 640x480");
            }
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(2);
            }
        }
    }
    Args {
        assets,
        server,
        size,
    }
}

struct App {
    args: Args,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    renderer: Option<SpriteRenderer>,
    text: Option<TextLayer>,
    client: Option<Client>,
    start: Instant,
    frames: u32,
    fps: f32,
    fps_time: Instant,
    scale: f64,
    /// Save a screenshot on the next frame.
    screenshot: Option<PathBuf>,
    /// Automation: `ZIRCON_SCREENSHOT=path[:seconds]` saves after a delay and exits.
    auto_screenshot: Option<(PathBuf, u64)>,
    /// `ZIRCON_HEADLESS=1`: never touch the swapchain (works on a locked
    /// screen); frames go to an offscreen texture that screenshots read.
    headless: bool,
    /// `ZIRCON_MOUSE=x,y`: hold the pointer there so headless screenshots
    /// can show hover states.
    forced_mouse: Option<(f32, f32)>,
}

impl App {
    fn frame(&mut self) {
        let (Some(gpu), Some(renderer), Some(text), Some(client), Some(window)) = (
            self.gpu.as_mut(),
            self.renderer.as_mut(),
            self.text.as_mut(),
            self.client.as_mut(),
            self.window.as_ref(),
        ) else {
            return;
        };
        let now = self.start.elapsed().as_millis() as u64;
        // ZIRCON_MOUSE=x,y parks the pointer for headless screenshots of
        // hover states (HUD tooltips).
        if let Some(at) = self.forced_mouse {
            client.input.mouse = at;
        }
        if self.frames == 0 && self.fps == 0.0 {
            tracing::info!(now, "first frame");
        }
        self.frames += 1;
        if self.fps_time.elapsed().as_secs_f32() >= 1.0 {
            self.fps = self.frames as f32 / self.fps_time.elapsed().as_secs_f32();
            self.frames = 0;
            self.fps_time = Instant::now();
        }
        let scale = self.scale as f32;
        let width = (gpu.config.width as f32 / scale) as i32;
        let height = (gpu.config.height as f32 / scale) as i32;

        let (surface, texture) = if self.headless {
            (None, gpu.offscreen_texture())
        } else {
            let surface = match gpu.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                    let (w, h) = (gpu.config.width, gpu.config.height);
                    gpu.resize(w, h);
                    return;
                }
                wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                    return;
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    tracing::error!("surface validation error");
                    return;
                }
            };
            let texture = surface.texture.clone();
            (Some(surface), texture)
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        renderer.set_scale(scale);
        client.frame(gpu, renderer, text, width, height, now, self.fps);
        text.prepare(gpu, scale);
        renderer.upload(gpu);

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        // Light pass: darkness colour plus additive light blobs, multiplied
        // over the world later in the scene pass (Zircon's LLayer).
        {
            let light_view = renderer.ensure_light_target(gpu);
            let [r, g, b] = client.light_clear();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("light"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &light_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: r as f64,
                            g: g as f64,
                            b: b as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            renderer.flush_light(gpu, &mut pass);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            // Layer by layer: each layer's sprites, then its text, so a
            // window's chrome covers everything drawn before it (names,
            // bubbles, the HUD and earlier windows) text included.
            for layer in 0..renderer.layers() {
                renderer.draw_layer(&mut pass, layer);
                text.render_layer(&mut pass, layer);
            }
        }
        renderer.end_frame();
        gpu.queue.submit([encoder.finish()]);
        if let Some((path, delay)) = &self.auto_screenshot {
            if now >= *delay {
                self.screenshot = Some(path.clone());
            }
        }
        if let Some(path) = self.screenshot.take() {
            let rgba = gpu.read_texture(&texture);
            match save_png(&path, gpu.config.width, gpu.config.height, &rgba) {
                Ok(()) => tracing::info!(path = %path.display(), "screenshot saved"),
                Err(e) => tracing::error!("screenshot failed: {e}"),
            }
            if self.auto_screenshot.is_some() {
                std::process::exit(0);
            }
        }
        if let Some(surface) = surface {
            window.pre_present_notify();
            gpu.queue.present(surface);
        }
        if let Some((width, height)) = client.game.take_pending_resolution() {
            let _ = window.request_inner_size(LogicalSize::new(width as f64, height as f64));
        }
        text.trim();
    }
}

fn save_png(path: &PathBuf, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(rgba)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(thread) = MainThreadMarker::new() else {
        return;
    };
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../icons/zircon.png"
    ));
    let data = NSData::with_bytes(bytes);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        tracing::warn!("could not decode Dock icon");
        return;
    };
    // AppKit owns the shared application and retains its icon image.
    unsafe { NSApplication::sharedApplication(thread).setApplicationIconImage(Some(&image)) };
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("ZirconRust")
            .with_inner_size(LogicalSize::new(
                self.args.size.0 as f64,
                self.args.size.1 as f64,
            ));
        let window = Arc::new(event_loop.create_window(attrs).expect("window"));
        #[cfg(target_os = "macos")]
        set_dock_icon();
        self.scale = window.scale_factor();
        let gpu = Gpu::new(window.clone()).expect("gpu");
        let renderer = SpriteRenderer::new(&gpu);
        let text = TextLayer::new(&gpu);
        let client = Client::new(Assets::new(&self.args.assets), self.args.server.clone());
        self.window = Some(window);
        self.gpu = Some(gpu);
        self.renderer = Some(renderer);
        self.text = Some(text);
        self.client = Some(client);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => self.scale = scale_factor,
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(c) = self.client.as_mut() {
                    c.input.mouse = (
                        (position.x / self.scale) as f32,
                        (position.y / self.scale) as f32,
                    );
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(c) = self.client.as_mut() {
                    let down = state == ElementState::Pressed;
                    match button {
                        MouseButton::Left => {
                            c.input.lmb_down = down;
                            if down {
                                c.input.lmb_pressed = true;
                            } else {
                                c.input.lmb_released = true;
                            }
                        }
                        MouseButton::Right => {
                            c.input.rmb_down = down;
                            if down {
                                c.input.rmb_pressed = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                if let Some(c) = self.client.as_mut() {
                    c.input.ctrl = m.state().control_key();
                    c.input.alt = m.state().alt_key();
                    c.input.command = m.state().super_key();
                    c.input.shift = m.state().shift_key();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::KeyS)
                            if self
                                .client
                                .as_ref()
                                .is_some_and(|c| c.input.ctrl && c.input.alt) =>
                        {
                            let name = format!("zircon-{}.png", chrono_stamp());
                            self.screenshot = Some(PathBuf::from(name));
                        }
                        PhysicalKey::Code(KeyCode::Backquote) => {
                            if let Some(c) = self.client.as_mut() {
                                c.debug = !c.debug;
                            }
                        }
                        _ => {}
                    }
                    if let Some(c) = self.client.as_mut() {
                        if let PhysicalKey::Code(code) = event.physical_key {
                            let f = match code {
                                KeyCode::F1 => Some(1),
                                KeyCode::F2 => Some(2),
                                KeyCode::F3 => Some(3),
                                KeyCode::F4 => Some(4),
                                KeyCode::F5 => Some(5),
                                KeyCode::F6 => Some(6),
                                KeyCode::F7 => Some(7),
                                KeyCode::F8 => Some(8),
                                KeyCode::F9 => Some(9),
                                KeyCode::F10 => Some(10),
                                KeyCode::F11 => Some(11),
                                KeyCode::F12 => Some(12),
                                _ => None,
                            };
                            let mac_slot = if cfg!(target_os = "macos")
                                && (c.input.alt || c.input.command)
                                && !c.input.ctrl
                                && !(c.input.alt && c.input.command)
                            {
                                match code {
                                    KeyCode::Digit1 => Some(1),
                                    KeyCode::Digit2 => Some(2),
                                    KeyCode::Digit3 => Some(3),
                                    KeyCode::Digit4 => Some(4),
                                    KeyCode::Digit5 => Some(5),
                                    KeyCode::Digit6 => Some(6),
                                    KeyCode::Digit7 => Some(7),
                                    KeyCode::Digit8 => Some(8),
                                    KeyCode::Digit9 => Some(9),
                                    KeyCode::Digit0 => Some(10),
                                    KeyCode::Minus => Some(11),
                                    KeyCode::Equal => Some(12),
                                    _ => None,
                                }
                            } else {
                                None
                            };
                            if let Some(slot) = f.or(mac_slot) {
                                c.input.fkey = Some(slot);
                            }
                            let digit = match code {
                                KeyCode::Digit1 => Some(0),
                                KeyCode::Digit2 => Some(1),
                                KeyCode::Digit3 => Some(2),
                                KeyCode::Digit4 => Some(3),
                                KeyCode::Digit5 => Some(4),
                                KeyCode::Digit6 => Some(5),
                                KeyCode::Digit7 => Some(6),
                                KeyCode::Digit8 => Some(7),
                                KeyCode::Digit9 => Some(8),
                                KeyCode::Digit0 => Some(9),
                                _ => None,
                            };
                            if digit.is_some()
                                && !event.repeat
                                && !c.input.command
                                && !c.input.alt
                                && !c.input.ctrl
                            {
                                c.input.digit = digit;
                            }
                            // Ctrl/Alt chords: `KeyH` -> 'h'.
                            if !event.repeat && (c.input.ctrl || c.input.alt) {
                                let name = format!("{code:?}");
                                if let Some(ch) =
                                    name.strip_prefix("Key").and_then(|s| s.chars().next())
                                {
                                    c.input.chord = Some(ch.to_ascii_lowercase());
                                }
                            }
                        }
                        match &event.logical_key {
                            Key::Named(NamedKey::Backspace) => c.input.backspace = true,
                            Key::Named(NamedKey::Enter) => c.input.enter = !event.repeat,
                            Key::Named(NamedKey::Tab) => c.input.tab = true,
                            Key::Named(NamedKey::Escape) => c.input.escape = true,
                            Key::Named(NamedKey::Delete) => c.input.delete = true,
                            Key::Named(NamedKey::ArrowUp) => c.input.up = true,
                            Key::Named(NamedKey::ArrowDown) => c.input.down = true,
                            Key::Named(NamedKey::ArrowLeft) => c.input.left = true,
                            Key::Named(NamedKey::ArrowRight) => c.input.right = true,
                            Key::Named(NamedKey::Space) => c.input.text.push(' '),
                            _ => {
                                if let Some(t) = &event.text {
                                    c.input.text.push_str(t);
                                }
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(c) = self.client.as_mut() {
                    c.input.wheel += match delta {
                        winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                        winit::event::MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                    };
                }
            }
            WindowEvent::RedrawRequested => self.frame(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
}

fn chrono_stamp() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    t.to_string()
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,wgpu_core=warn,wgpu_hal=warn,naga=warn".into()),
        )
        .init();
    let args = parse_args();
    tracing::info!(assets = %args.assets.display(), server = args.server, "starting client");
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        args,
        window: None,
        gpu: None,
        renderer: None,
        text: None,
        client: None,
        start: Instant::now(),
        frames: 0,
        fps: 0.0,
        fps_time: Instant::now(),
        scale: 1.0,
        screenshot: None,
        headless: std::env::var_os("ZIRCON_HEADLESS").is_some(),
        forced_mouse: std::env::var("ZIRCON_MOUSE").ok().and_then(|v| {
            let (x, y) = v.split_once(',')?;
            Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
        }),
        auto_screenshot: std::env::var("ZIRCON_SCREENSHOT").ok().map(|v| {
            let (path, secs) = match v.rsplit_once(':') {
                Some((p, s)) if s.parse::<f64>().is_ok() => {
                    (p.to_string(), s.parse::<f64>().unwrap())
                }
                _ => (v.clone(), 3.0),
            };
            (PathBuf::from(path), (secs * 1000.0) as u64)
        }),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
