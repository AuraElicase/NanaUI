//! Host-owned wgpu Surface on top of `ANativeWindow` (Vulkan).
//!
//! Each frame clears the surface and hands it to the Scene painter; the
//! Runtime document paints the whole window, background included.

use android_activity::AndroidApp;
use raw_window_handle::{AndroidNdkWindowHandle, DisplayHandle, RawWindowHandle, WindowHandle};
use wgpu::{
    CompositeAlphaMode, CurrentSurfaceTexture, Instance, PresentMode, Surface,
    SurfaceConfiguration, SurfaceTargetUnsafe, TextureFormat, TextureUsages,
};

use nana_ui::{FrameContext, GpuContext, GpuRenderTarget};

pub struct GpuSurface {
    pub surface: Surface<'static>,
    pub gpu: GpuContext,
    pub config: SurfaceConfiguration,
    pub format: TextureFormat,
    _instance: Instance,
}

impl GpuSurface {
    /// Create a Vulkan-capable surface from the current native window.
    pub fn new(app: &AndroidApp, width: u32, height: u32) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);

        let native = app
            .native_window()
            .ok_or_else(|| "ANativeWindow missing".to_string())?;

        let instance = Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        // SAFETY: Native window is alive for the duration of this Surface while
        // the activity holds the window; runtime drops Surface on Destroyed.
        let surface = unsafe {
            let display = DisplayHandle::android();
            let window = AndroidNdkWindowHandle::new(native.ptr().cast());
            // SAFETY: the ANativeWindow is retained by `native` for this
            // surface creation call and remains alive for the Surface.
            let window = WindowHandle::borrow_raw(RawWindowHandle::AndroidNdk(window));
            instance.create_surface_unsafe(SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(display.as_raw()),
                raw_window_handle: window.as_raw(),
            })
        }
        .map_err(|e| format!("create_surface: {e}"))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("request_adapter: {e}"))?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("nana-android-host"),
            required_features: wgpu::Features::empty(),
            // SceneWgpuPainter exceeds downlevel limits (fragment storage buffers,
            // 16 inter-stage variables); ask for what the adapter has, never more.
            required_limits: adapter.limits(),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|e| format!("request_device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        // Resolve the host's default SDR profile from the complete table. In
        // particular, do not assume that the first format is sRGB: wgpu 30
        // can advertise formats that are only valid in an explicit colour
        // space (and those capabilities change when the window moves).
        let (format, color_space) = caps
            .format_capabilities
            .iter()
            .find_map(|row| {
                row.color_spaces
                    .contains(wgpu::SurfaceColorSpaces::SRGB)
                    .then_some((row.format, wgpu::SurfaceColorSpace::Srgb))
            })
            .or_else(|| {
                caps.formats
                    .first()
                    .copied()
                    .map(|f| (f, wgpu::SurfaceColorSpace::Srgb))
            })
            .ok_or_else(|| "no surface formats".to_string())?;

        let alpha = if caps.alpha_modes.contains(&CompositeAlphaMode::Opaque) {
            CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes
                .first()
                .copied()
                .unwrap_or(CompositeAlphaMode::Auto)
        };

        let present = if caps.present_modes.contains(&PresentMode::Fifo) {
            PresentMode::Fifo
        } else {
            caps.present_modes
                .first()
                .copied()
                .unwrap_or(PresentMode::Fifo)
        };

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space,
            width,
            height,
            present_mode: present,
            desired_maximum_frame_latency: 2,
            alpha_mode: alpha,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            gpu: GpuContext::from_wgpu(adapter, device, queue),
            config,
            format,
            _instance: instance,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        let width = width.max(1);
        let height = height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface
            .configure(self.gpu.wgpu().device(), &self.config);
    }

    /// Clear the surface, then let `overlay` paint the Scene, in one submit.
    pub fn present_with_overlay(
        &mut self,
        mut overlay: impl FnMut(&GpuRenderTarget, &mut FrameContext) -> Result<(), String>,
    ) -> Result<(), String> {
        let frame = match self.surface.get_current_texture() {
            CurrentSurfaceTexture::Success(frame) | CurrentSurfaceTexture::Suboptimal(frame) => {
                frame
            }
            CurrentSurfaceTexture::Timeout | CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            CurrentSurfaceTexture::Outdated => {
                self.surface
                    .configure(self.gpu.wgpu().device(), &self.config);
                return Err("surface outdated (reconfigured)".into());
            }
            CurrentSurfaceTexture::Lost => return Err("surface lost".into()),
            CurrentSurfaceTexture::Validation => return Err("surface validation".into()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let target = GpuRenderTarget::from_wgpu(
            &self.gpu,
            view.clone(),
            self.format,
            [frame.texture.width(), frame.texture.height()],
        );
        let mut recording = self.gpu.begin_frame("nana-android frame");
        let encoder = recording.wgpu_encoder();
        // The document's root covers the surface; the clear only shows while
        // a frame has nothing drawn yet.
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nana-android clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // The frame is dropped before the surface texture on failure.
        overlay(&target, &mut recording)?;
        recording.submit();
        self.gpu.wgpu().queue().present(frame);
        Ok(())
    }
}
