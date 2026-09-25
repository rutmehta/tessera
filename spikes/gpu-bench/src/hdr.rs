//! HDR surface probe: a hidden 64x64 winit window, a wgpu (Metal) surface on it,
//! then `Surface::configure` with `TextureFormat::Rgba16Float` and each of
//! `SurfaceColorSpace::ExtendedSrgbLinear` / `ExtendedDisplayP3`, validation
//! errors captured with an error scope. Also records `Surface::get_capabilities`
//! (`format_capabilities` for Rgba16Float) and `Surface::display_hdr_info`.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub struct HdrResult {
    pub extended_srgb_linear: bool,
    pub extended_display_p3: bool,
    pub notes: String,
}

#[derive(Default)]
struct App {
    result: Option<HdrResult>,
}

fn probe(window: Arc<Window>) -> HdrResult {
    let mut notes = Vec::new();
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(desc);
    let surface = match instance.create_surface(window.clone()) {
        Ok(s) => s,
        Err(e) => {
            return HdrResult {
                notes: format!("create_surface failed: {e}"),
                ..Default::default()
            }
        }
    };
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        ..Default::default()
    })) {
        Ok(a) => a,
        Err(e) => {
            return HdrResult {
                notes: format!("request_adapter failed: {e}"),
                ..Default::default()
            }
        }
    };
    let (device, _queue) = match pollster::block_on(adapter.request_device(&Default::default())) {
        Ok(d) => d,
        Err(e) => {
            return HdrResult {
                notes: format!("request_device failed: {e}"),
                ..Default::default()
            }
        }
    };

    let caps = surface.get_capabilities(&adapter);
    let f16 = wgpu::TextureFormat::Rgba16Float;
    notes.push(format!(
        "Surface::get_capabilities: Rgba16Float in `formats` (Auto-usable) = {}; \
         format_capabilities color_spaces for Rgba16Float = {:?}",
        caps.formats.contains(&f16),
        caps.color_spaces(f16)
    ));
    let hdr = surface.display_hdr_info(&adapter);
    notes.push(format!(
        "Surface::display_hdr_info -> DisplayHdrInfo {{ headroom: {:?}, coarse: {:?}, luminance: {:?}, \
         chromaticity: {:?}, bits_per_color: {:?} }}; tone_map_headroom() = {:?}",
        hdr.headroom, hdr.coarse, hdr.luminance, hdr.chromaticity, hdr.bits_per_color,
        hdr.tone_map_headroom()
    ));

    // Preserve the actual display query even if native configure crashes.
    eprintln!("{}", notes.join("\n"));
    let size = window.inner_size();
    let mut try_space = |cs: wgpu::SurfaceColorSpace| -> bool {
        let advertised = caps
            .color_spaces(f16)
            .contains(cs.to_color_spaces().unwrap());
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: f16,
            color_space: cs,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &config);
        let err = pollster::block_on(scope.pop());
        let ok = err.is_none();
        let acquire = if ok {
            match surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(t)
                | wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                    let fmt = t.texture.format();
                    drop(t);
                    format!("acquired frame ({fmt:?})")
                }
                other => format!("acquire: {}", variant_name(&other)),
            }
        } else {
            "not attempted".into()
        };
        notes.push(format!(
            "configure(Rgba16Float, {cs:?}): advertised={advertised}, {} ; {acquire}",
            match &err {
                None => "OK".to_string(),
                Some(e) => format!("validation error: {e}"),
            }
        ));
        ok
    };
    let mode = std::env::args().nth(1).unwrap_or_default();
    let a = mode == "--hdr-srgb" && try_space(wgpu::SurfaceColorSpace::ExtendedSrgbLinear);
    let b = mode == "--hdr-p3" && try_space(wgpu::SurfaceColorSpace::ExtendedDisplayP3);
    notes.push(format!(
        "window: hidden, {}x{} physical px",
        size.width, size.height
    ));
    HdrResult {
        extended_srgb_linear: a,
        extended_display_p3: b,
        notes: notes.join("\n"),
    }
}

fn variant_name(t: &wgpu::CurrentSurfaceTexture) -> String {
    let s = format!("{t:?}");
    s.split(['(', ' ', '{']).next().unwrap_or("?").to_string()
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("gpu-bench HDR probe")
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(64.0, 64.0));
        self.result = Some(match event_loop.create_window(attrs) {
            Ok(w) => probe(Arc::new(w)),
            Err(e) => HdrResult {
                notes: format!("create_window failed: {e}"),
                ..Default::default()
            },
        });
        event_loop.exit();
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

pub fn run_single() -> HdrResult {
    let event_loop = match EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            return HdrResult {
                notes: format!("EventLoop::new failed: {e}"),
                ..Default::default()
            }
        }
    };
    let mut app = App::default();
    if let Err(e) = event_loop.run_app(&mut app) {
        return HdrResult {
            notes: format!("run_app failed: {e}"),
            ..Default::default()
        };
    }
    app.result.unwrap_or_else(|| HdrResult {
        notes: "event loop exited before resumed".into(),
        ..Default::default()
    })
}

/// Native surface configuration may abort below Rust's error-scope boundary.
/// Run each real configuration independently, never infer support from caps.
pub fn run() -> HdrResult {
    let mut result = HdrResult::default();
    for mode in ["--hdr-srgb", "--hdr-p3"] {
        let output = std::env::current_exe()
            .and_then(|exe| std::process::Command::new(exe).arg(mode).output());
        let note = match output {
            Ok(out) if out.status.success() => {
                match serde_json::from_slice::<HdrResult>(&out.stdout) {
                    Ok(probe) => {
                        result.extended_srgb_linear |= probe.extended_srgb_linear;
                        result.extended_display_p3 |= probe.extended_display_p3;
                        probe.notes
                    }
                    Err(e) => format!("{mode}: invalid probe result: {e}"),
                }
            }
            Ok(out) => format!(
                "{mode}: native probe failed ({}). Captured display query:\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ),
            Err(e) => format!("{mode}: could not launch probe: {e}"),
        };
        result.notes.push_str(&note);
        result.notes.push('\n');
    }
    result
}
