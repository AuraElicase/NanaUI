//! Host-only Windows implementation; no OpenVR types enter NanaUI APIs.
//! ABI: Valve openvr_capi.h at 0924064316de3effbcd1acf1e309182a2deb1c05,
//! https://github.com/ValveSoftware/openvr/blob/0924064316de3effbcd1acf1e309182a2deb1c05/headers/openvr_capi.h
//! IVROverlay_028 is requested exactly; other layouts are never guessed.

use std::ffi::{CString, c_char, c_void};
use std::mem::ManuallyDrop;
use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::ProbeError;
use nana_gpu::{
    GpuContext, GpuTextureFormat, NativeExportOutcome, NativeExportPool, NativeFrameToken,
};
use nana_ui::NanaTextShaper;
use nana_ui::ScenePresentationProfile;
use nana_ui::output::{
    ExternalRenderOutcome, OutputAlpha, OutputConsumer, OutputConsumerId, OutputConsumerKind,
    OutputOwnership, OutputTextAa, PresentationDriver, PresentationId, PresenterCapabilities,
    RenderTargetRequirements,
};
use nana_ui::runtime::{DocumentId, LayoutViewport, RuntimeDocument, Text};
use windows::Win32::Foundation::{HANDLE, HMODULE, LUID};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_FENCE_FLAG_NONE,
    D3D11_RESOURCE_MISC_SHARED, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11CreateDevice, ID3D11Device, ID3D11Device1, ID3D11Device5, ID3D11DeviceContext,
    ID3D11DeviceContext4, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, IDXGIAdapter, IDXGIFactory4, IDXGIResource,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{Interface, PCSTR, PCWSTR};

const EXTENT: [u32; 2] = [128, 96];
const CAPACITY: usize = 3;
const ATTEMPTS: usize = 240;
const WARMUP_FRAMES: usize = 30;
// Pinned openvr.h: this flag selects premultiplied texture alpha composition.
const OVERLAY_IS_PREMULTIPLIED: i32 = 1 << 21;
type SetOverlayFlag = unsafe extern "system" fn(u64, i32, bool) -> i32;
type Result<T> = std::result::Result<T, String>;

#[repr(C)]
struct Texture {
    handle: *mut c_void,
    api: i32,         // ETextureType: DirectX pointer or DXGISharedHandle
    color_space: i32, // EColorSpace::Gamma
}

#[repr(C)]
struct Matrix34 {
    rows: [[f32; 4]; 3],
}

// Exact C function-table offsets for IVROverlay_028. Unused entries remain
// pointer-sized; no C++ object/vtable or private SteamVR ABI is accessed.
#[repr(C)]
struct OverlayTable {
    unused_0: [usize; 1],
    create: unsafe extern "system" fn(*const c_char, *const c_char, *mut u64) -> i32,
    unused_2: [usize; 1],
    destroy: unsafe extern "system" fn(u64) -> i32,
    unused_4: [usize; 7],
    set_flag: SetOverlayFlag,
    unused_12: [usize; 10],
    width: unsafe extern "system" fn(u64, f32) -> i32,
    unused_23: [usize; 12],
    transform: unsafe extern "system" fn(u64, u32, *const Matrix34) -> i32,
    unused_36: [usize; 7],
    show: unsafe extern "system" fn(u64) -> i32,
    unused_44: [usize; 16],
    set_texture: unsafe extern "system" fn(u64, *const Texture) -> i32,
    clear: unsafe extern "system" fn(u64) -> i32,
}

struct OpenVr {
    _module: HMODULE,
    table: &'static OverlayTable,
    shutdown: unsafe extern "C" fn(),
    overlay: u64,
}

fn vr_ok(code: i32, operation: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!("OpenVR {operation}: error {code}"))
    }
}

fn configure_premultiplied(set_flag: SetOverlayFlag, overlay: u64) -> Result<()> {
    // SAFETY: the exact IVROverlay_028 table supplies this function pointer;
    // the overlay was successfully created by that same runtime instance.
    vr_ok(
        unsafe { set_flag(overlay, OVERLAY_IS_PREMULTIPLIED, true) },
        "SetOverlayFlag(IsPremultiplied)",
    )
}

// Pinned EVRInitError::VRInitError_Init_InterfaceNotFound is 105. Other
// failures, including success plus a null interface, are runtime faults.
fn interface_failure(code: i32, null_interface: bool) -> ProbeError {
    let message = format!("OpenVR IVROverlay_028: error {code}, null={null_interface}");
    if code == 105 {
        ProbeError::Unsupported(message)
    } else {
        ProbeError::Failed(message)
    }
}

impl OpenVr {
    fn load(path: &std::path::Path) -> std::result::Result<Self, ProbeError> {
        if !path.is_absolute() || !path.is_file() {
            return Err(ProbeError::Unsupported(
                "--openvr-dll must name an existing absolute SDK DLL path".into(),
            ));
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: trusted, explicitly selected SDK library; loading does not
        // search the working directory or PATH for dependent DLLs.
        let module = unsafe {
            LoadLibraryExW(
                PCWSTR(wide.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .map_err(|error| ProbeError::Unsupported(error.to_string()))?;
        let symbol = |name: &'static [u8]| {
            unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }.ok_or_else(|| {
                ProbeError::Unsupported(format!("OpenVR SDK export missing: {:?}", name))
            })
        };
        // SAFETY: these are the published C exports of openvr_api.dll.
        let init: unsafe extern "C" fn(*mut i32, i32) -> u32 =
            unsafe { std::mem::transmute(symbol(b"VR_InitInternal\0")?) };
        let get: unsafe extern "C" fn(*const c_char, *mut i32) -> *const OverlayTable =
            unsafe { std::mem::transmute(symbol(b"VR_GetGenericInterface\0")?) };
        let shutdown: unsafe extern "C" fn() =
            unsafe { std::mem::transmute(symbol(b"VR_ShutdownInternal\0")?) };
        let mut error = 0;
        unsafe { init(&mut error, 2) }; // VRApplication_Overlay
        vr_ok(error, "VR_InitInternal")?;
        let table = unsafe { get(c"FnTable:IVROverlay_028".as_ptr(), &mut error) };
        if error != 0 || table.is_null() {
            unsafe { shutdown() };
            return Err(interface_failure(error, table.is_null()));
        }
        let table = unsafe { &*table };
        let mut overlay = 0;
        let key = CString::new(format!("dev.nana.issue242.{}", std::process::id())).unwrap();
        vr_ok(
            unsafe {
                (table.create)(
                    key.as_ptr(),
                    c"NanaUI GPU output reference".as_ptr(),
                    &mut overlay,
                )
            },
            "CreateOverlay",
        )?;
        let runtime = Self {
            _module: module,
            table,
            shutdown,
            overlay,
        };
        configure_premultiplied(table.set_flag, overlay)?;
        vr_ok(
            unsafe { (table.width)(overlay, 0.6) },
            "SetOverlayWidthInMeters",
        )?;
        let matrix = Matrix34 {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, -1.0],
            ],
        };
        vr_ok(
            unsafe { (table.transform)(overlay, 0, &matrix) }, // tracked HMD
            "SetOverlayTransformTrackedDeviceRelative",
        )?;
        vr_ok(unsafe { (table.show)(overlay) }, "ShowOverlay")?;
        Ok(runtime)
    }

    fn present(&self, texture: &ID3D11Texture2D) -> Result<()> {
        let texture = Texture {
            handle: texture.as_raw(),
            api: 0,
            color_space: 1,
        };
        vr_ok(
            unsafe { (self.table.set_texture)(self.overlay, &texture) },
            "SetOverlayTexture",
        )
    }

    fn present_shared(&self, handle: HANDLE) -> Result<()> {
        let texture = Texture {
            handle: handle.0,
            api: 5,
            color_space: 1,
        };
        vr_ok(
            unsafe { (self.table.set_texture)(self.overlay, &texture) },
            "SetOverlayTexture(DXGISharedHandle)",
        )
    }
}

impl Drop for OpenVr {
    fn drop(&mut self) {
        unsafe {
            (self.table.clear)(self.overlay);
            (self.table.destroy)(self.overlay);
            (self.shutdown)();
        }
        // Neither shutdown nor clear is a GPU retirement receipt. The DLL
        // and independent immutable consumer pool live until process exit.
    }
}

struct Pending {
    texture: ID3D11Texture2D,
    done: u64,
}

struct Consumer {
    luid: i64,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    context4: ID3D11DeviceContext4,
    completed: ID3D11Fence,
    next: u64,
    pending: Vec<Pending>,
    // Never overwritten or released based on an OpenVR API return. This
    // bounded consumer-owned memory survives even error/unwind/disconnect.
    published: Vec<ID3D11Texture2D>,
    immutable: bool,
    shared: Option<(ID3D11Texture2D, HANDLE)>,
    shared_presented: bool,
    updates_completed: usize,
    copies: u64,
    copy_bytes: u64,
    target_allocations: u64,
    native_calls_ns: Vec<u64>,
    copy_calls_ns: Vec<u64>,
    opened: Option<(u64, Vec<ID3D11Texture2D>, ID3D11Fence)>,
}

impl Consumer {
    fn new(luid: i64, immutable: bool) -> Result<Self> {
        unsafe {
            let factory: IDXGIFactory4 =
                CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).map_err(|e| e.to_string())?;
            let adapter: IDXGIAdapter = factory
                .EnumAdapterByLuid(LUID {
                    LowPart: luid as u32,
                    HighPart: (luid >> 32) as i32,
                })
                .map_err(|e| e.to_string())?;
            let (mut device, mut context) = (None, None);
            D3D11CreateDevice(
                &adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .map_err(|e| e.to_string())?;
            let device = device.ok_or("D3D11 device")?;
            let context = context.ok_or("D3D11 context")?;
            let device5: ID3D11Device5 = device.cast().map_err(|e| e.to_string())?;
            let mut completed: Option<ID3D11Fence> = None;
            device5
                .CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut completed)
                .map_err(|e| e.to_string())?;
            Ok(Self {
                luid,
                context4: context.cast().map_err(|e| e.to_string())?,
                device,
                context,
                completed: completed.ok_or("D3D11 completion fence")?,
                next: 1,
                pending: Vec::new(),
                published: Vec::new(),
                immutable,
                shared: None,
                shared_presented: false,
                updates_completed: 0,
                copies: 0,
                copy_bytes: 0,
                target_allocations: 0,
                native_calls_ns: Vec::new(),
                copy_calls_ns: Vec::new(),
                opened: None,
            })
        }
    }

    fn full(&self) -> bool {
        self.pending.len()
            + if self.immutable {
                self.published.len()
            } else {
                0
            }
            >= CAPACITY
    }

    fn stage(&mut self, mut token: NativeFrameToken) -> Result<()> {
        let started = Instant::now();
        if self.full() {
            return Err(
                "consumer immutable pool is full; continuous retirement unsupported".into(),
            );
        }
        if token.adapter_luid() != self.luid {
            return Err("native token adapter changed; recreate consumer".into());
        }
        unsafe {
            if self
                .opened
                .as_ref()
                .is_none_or(|(generation, _, _)| *generation != token.pool_generation())
            {
                let device1: ID3D11Device1 = self.device.cast().map_err(|e| e.to_string())?;
                let device5: ID3D11Device5 = self.device.cast().map_err(|e| e.to_string())?;
                let mut textures = Vec::new();
                for slot in 0..nana_gpu::NATIVE_EXPORT_SLOTS {
                    textures.push(
                        device1
                            .OpenSharedResource1::<ID3D11Texture2D>(HANDLE(
                                token
                                    .texture_handle(slot)
                                    .ok_or("native slot")?
                                    .as_raw_handle(),
                            ))
                            .map_err(|e| e.to_string())?,
                    );
                }
                let mut fence: Option<ID3D11Fence> = None;
                device5
                    .OpenSharedFence(HANDLE(token.fence_handle().as_raw_handle()), &mut fence)
                    .map_err(|e| e.to_string())?;
                self.opened = Some((
                    token.pool_generation(),
                    textures,
                    fence.ok_or("shared fence")?,
                ));
            }
            let (_, textures, fence) = self.opened.as_ref().unwrap();
            let shared = &textures[token.slot()];
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            shared.GetDesc(&mut desc);
            if [desc.Width, desc.Height] != token.extent() {
                return Err("native token extent does not match imported resource".into());
            }
            let desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: if self.immutable {
                    0
                } else {
                    D3D11_RESOURCE_MISC_SHARED.0 as u32
                },
                ..desc
            };
            let destination = if let Some((texture, _)) = &self.shared {
                texture.clone()
            } else {
                let mut destination = None;
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut destination))
                    .map_err(|e| e.to_string())?;
                let texture = destination.ok_or("consumer texture")?;
                self.target_allocations += 1;
                if !self.immutable {
                    let resource: IDXGIResource = texture.cast().map_err(|e| e.to_string())?;
                    // Legacy DXGI shared handles follow the resource's lifetime;
                    // they are not NT handles to pass to CloseHandle.
                    let handle = resource.GetSharedHandle().map_err(|e| e.to_string())?;
                    self.shared = Some((texture.clone(), handle));
                }
                texture
            };
            // A GPU timeline wait, never a CPU fence wait. The producer token
            // may retire once this independent copy has finished, regardless
            // of how long SteamVR retains the consumer-owned destination.
            self.context4
                .Wait(fence, token.ready_value())
                .map_err(|e| e.to_string())?;
            token.accept_release();
            self.context.CopyResource(&destination, shared);
            self.context4
                .Signal(fence, token.release_value())
                .map_err(|e| e.to_string())?;
            let done = self.next;
            self.next += 1;
            self.context4
                .Signal(&self.completed, done)
                .map_err(|e| e.to_string())?;
            self.context.Flush();
            self.copies += 1;
            self.copy_bytes += desc.Width as u64 * desc.Height as u64 * 4;
            self.pending.push(Pending {
                texture: destination,
                done,
            });
        }
        self.copy_calls_ns.push(started.elapsed().as_nanos() as u64);
        Ok(())
    }

    fn poll(&mut self, runtime: &OpenVr) -> Result<()> {
        let completed = unsafe { self.completed.GetCompletedValue() };
        if completed == u64::MAX {
            return Err("D3D11 consumer device removed".into());
        }
        while self
            .pending
            .first()
            .is_some_and(|pending| pending.done <= completed)
        {
            let pending = self.pending.remove(0);
            self.updates_completed += 1;
            if self.immutable {
                // Retain before calling foreign code, including its error path.
                self.published.push(pending.texture);
                let start = Instant::now();
                let result = runtime.present(self.published.last().unwrap());
                self.native_calls_ns.push(start.elapsed().as_nanos() as u64);
                result?;
            } else if !self.shared_presented {
                // The pinned header explicitly permits only atomic
                // CopyResource/resolve updates to DXGISharedHandle overlays.
                // Every later write is exactly that operation, and this
                // resource's identity is never retired or reallocated.
                let start = Instant::now();
                let result = runtime.present_shared(self.shared.as_ref().unwrap().1);
                self.native_calls_ns.push(start.elapsed().as_nanos() as u64);
                result?;
                self.shared_presented = true;
            }
        }
        Ok(())
    }
}

impl Drop for Consumer {
    fn drop(&mut self) {
        // OpenVR has no release-fence proof in this reference. Keep the bounded
        // imported-resource lifetime until process termination, including on
        // errors. This is deliberately not a reusable library allocator.
        for texture in self.published.drain(..) {
            std::mem::forget(texture);
        }
        for pending in self.pending.drain(..) {
            std::mem::forget(pending.texture);
        }
        if let Some((texture, _)) = self.shared.take() {
            std::mem::forget(texture);
        }
        std::mem::forget(self.device.clone());
        std::mem::forget(self.context.clone());
    }
}

fn gpu() -> std::result::Result<GpuContext, ProbeError> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .map_err(|e| ProbeError::Unsupported(e.to_string()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| e.to_string())?;
    Ok(GpuContext::from_wgpu_with_loss_tracking(
        adapter, device, queue,
    ))
}

fn quantile(values: &[u64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    Some(values[((values.len() - 1) as f64 * q).ceil() as usize] as f64 / 1_000_000.0)
}

fn timing(values: &[u64]) -> serde_json::Value {
    serde_json::json!({"status": if values.is_empty() {"skipped"} else {"measured"},
        "samples": values.len(), "p50": quantile(values, 0.5),
        "p95": quantile(values, 0.95), "p99": quantile(values, 0.99)})
}

pub(crate) fn run() -> std::result::Result<serde_json::Value, ProbeError> {
    let args: Vec<_> = std::env::args().collect();
    let immutable = args.iter().any(|arg| arg == "--immutable-snapshots");
    let dll = args
        .windows(2)
        .find(|pair| pair[0] == "--openvr-dll")
        .map(|pair| PathBuf::from(&pair[1]))
        .or_else(|| std::env::var_os("NANA_OPENVR_DLL").map(PathBuf::from))
        .ok_or_else(|| {
            ProbeError::Unsupported("--openvr-dll <absolute SDK DLL path> is required".into())
        })?;
    let runtime = OpenVr::load(&dll)?;
    let gpu = gpu()?;
    let mut native = NativeExportPool::new(&gpu, EXTENT).map_err(|error| match error {
        nana_gpu::NativeExportError::Unsupported(reason) if !gpu.is_lost() => {
            ProbeError::Unsupported(reason.into())
        }
        error => ProbeError::Failed(error.to_string()),
    })?;
    // ManuallyDrop is intentional: normal and exceptional exits cannot drop
    // foreign-accessed resources merely because an SDK call returned.
    let mut consumer = ManuallyDrop::new(Consumer::new(
        native.adapter_luid().ok_or("native adapter")?,
        immutable,
    )?);
    let mut driver = PresentationDriver::new(&gpu);
    driver
        .configure(&[OutputConsumer {
            id: OutputConsumerId(1),
            kind: OutputConsumerKind::External,
            requirements: RenderTargetRequirements {
                presentation: PresentationId(1),
                viewport_revision: 1,
                extent: EXTENT,
                profile: ScenePresentationProfile::sdr(GpuTextureFormat::BGRA8_UNORM),
                alpha: OutputAlpha::GammaPremultiplied,
                text_aa: OutputTextAa::Grayscale,
                device_generation: gpu.generation(),
                ownership: OutputOwnership::SameDevice,
                persistent: true,
            },
            capabilities: PresenterCapabilities {
                sampled: true,
                copy_destination: true,
                ..Default::default()
            },
        }])
        .map_err(|e| e.to_string())?;
    let id = DocumentId::new(1).unwrap();
    let mut document = RuntimeDocument::new(id);
    document
        .context_mut()
        .create_component(id, Text::new("NanaUI native GPU overlay"))
        .map_err(|e| e.to_string())?;
    document
        .flush(
            LayoutViewport::new(128.0, 96.0),
            &mut NanaTextShaper::default(),
        )
        .map_err(|e| e.to_string())?;
    if document.scene().primitive_count() == 0 {
        return Err("OpenVR source workload has no drawable primitives".into());
    }
    let scene = document.shared_scene();
    let mut deferred = 0;
    let mut produced = 0;
    let mut attempts = 0;
    let mut total_attempts = 0;
    let mut measurement_started = immutable;
    let mut warmup_attempts = 0;
    let mut warmup_frames = 0;
    let mut native_baseline = 0;
    let mut copy_baseline = 0;
    let mut bytes_baseline = 0;
    let mut completed_baseline = 0;
    let mut allocation_baseline = 0;
    let mut setup_native_calls_ns = Vec::new();
    let mut exported_revision = None;
    let mut completed_source_revision = None;
    let mut producer_calls_ns = Vec::new();
    let mut native_record_calls_ns = Vec::new();
    let mut producer_ui_renders = 0;
    let mut producer_ui_submissions = 0;
    let mut producer_draw_calls = 0;
    let mut last_revision = 0;
    let mut last_color = [0.0; 4];
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut next_tick = Instant::now();
    loop {
        driver.poll();
        consumer.poll(&runtime)?;
        // Drain the 30 actual foreign copies before snapshotting counters.
        // This phase boundary does not turn consumer readiness into a UI wait.
        if !measurement_started && produced >= WARMUP_FRAMES && consumer.pending.is_empty() {
            warmup_attempts = attempts;
            warmup_frames = produced;
            native_baseline = native.issued();
            copy_baseline = consumer.copies;
            bytes_baseline = consumer.copy_bytes;
            completed_baseline = consumer.updates_completed;
            allocation_baseline = consumer.target_allocations;
            setup_native_calls_ns = std::mem::take(&mut consumer.native_calls_ns);
            consumer.copy_calls_ns.clear();
            producer_calls_ns.clear();
            native_record_calls_ns.clear();
            producer_ui_renders = 0;
            producer_ui_submissions = 0;
            producer_draw_calls = 0;
            attempts = 0;
            produced = 0;
            deferred = 0;
            measurement_started = true;
        }
        let source_done = if immutable {
            attempts >= ATTEMPTS
        } else if measurement_started {
            produced >= ATTEMPTS
        } else {
            produced >= WARMUP_FRAMES
        };
        if measurement_started && source_done && consumer.pending.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("OpenVR host progress deadline expired".into());
        }
        if !source_done && Instant::now() >= next_tick {
            next_tick = Instant::now() + Duration::from_millis(16);
            let revision = total_attempts as u64;
            let color = [
                [0.9, 0.1, 0.2, 1.0],
                [0.1, 0.9, 0.2, 1.0],
                [0.1, 0.2, 0.9, 1.0],
            ][total_attempts % 3];
            attempts += 1;
            total_attempts += 1;
            // Exactly one source update per host tick. A slow consumer never
            // postpones this UI clock or starts an inner readiness-wait loop.
            let started = Instant::now();
            let outcomes = driver
                .render_scene(&scene, [128.0, 96.0], revision, color, None, None)
                .map_err(|e| e.to_string())?;
            producer_calls_ns.push(started.elapsed().as_nanos() as u64);
            let rendered = driver
                .work()
                .output_ui_renders
                .ok_or("missing producer render observation")?;
            producer_ui_renders += rendered;
            producer_ui_submissions += driver
                .work()
                .output_submissions
                .ok_or("missing producer submission observation")?;
            if rendered > 0 {
                producer_draw_calls += driver
                    .work()
                    .draw_calls
                    .ok_or("missing actual painter draw observation")?;
            }
            if let ExternalRenderOutcome::Submitted {
                content_revision, ..
            }
            | ExternalRenderOutcome::Reused {
                content_revision, ..
            } = outcomes[0]
            {
                completed_source_revision = Some(content_revision);
                last_revision = revision;
                last_color = color;
            }
            driver.poll();
            if consumer.full() {
                deferred += 1;
            } else {
                match driver.sample(OutputConsumerId(1)) {
                    Ok(lease) if exported_revision != Some(lease.content_revision()) => {
                        if let Some(mut frame) = gpu.try_begin_frame("OpenVR native export") {
                            let native_started = Instant::now();
                            match native
                                .stage(&mut frame, lease.texture().map_err(|e| e.to_string())?)
                                .map_err(|e| e.to_string())?
                            {
                                NativeExportOutcome::Staged(plan) => {
                                    let revision = lease.content_revision();
                                    frame.keep_alive_until_complete(lease);
                                    let _submission = frame.submit();
                                    let token = native.finish(plan).map_err(|e| e.to_string())?;
                                    native_record_calls_ns
                                        .push(native_started.elapsed().as_nanos() as u64);
                                    consumer.stage(token)?;
                                    exported_revision = Some(revision);
                                    produced += 1;
                                }
                                NativeExportOutcome::Deferred(_) => {
                                    frame.discard();
                                    deferred += 1;
                                }
                            }
                        } else {
                            deferred += 1;
                        }
                    }
                    Ok(_) => {
                        deferred += 1;
                    }
                    Err(nana_ui::output::PresentationError::Surface(
                        nana_ui::output::ExternalSurfaceError::NoCompletedFrame,
                    )) => {
                        deferred += 1;
                    }
                    Err(error) => return Err(error.to_string().into()),
                }
            }
        }
        // Standalone event-loop pacing, outside the nonblocking producer tick.
        std::thread::sleep(Duration::from_millis(1));
    }
    // Measurement phase barrier only: let the last source submit complete.
    // This does not execute, retry or delay any application update.
    loop {
        driver.poll();
        if let Ok(source) = driver.sample(OutputConsumerId(1))
            && Some(source.content_revision()) == completed_source_revision
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("source phase barrier deadline expired".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let static_before = consumer.copies;
    let native_before = native.issued();
    let mut static_ui_renders = 0;
    let mut static_submissions = 0;
    for _ in 0..ATTEMPTS {
        let outcomes = driver
            .render_scene(&scene, [128.0, 96.0], last_revision, last_color, None, None)
            .map_err(|e| e.to_string())?;
        if !matches!(outcomes[0], ExternalRenderOutcome::Reused { .. }) {
            return Err("unchanged source was not reused".into());
        }
        static_ui_renders += driver
            .work()
            .output_ui_renders
            .ok_or("missing static render observation")?;
        static_submissions += driver
            .work()
            .output_submissions
            .ok_or("missing static submission observation")?;
        consumer.poll(&runtime)?;
    }
    if producer_draw_calls == 0 {
        return Err("OpenVR source workload recorded zero draws".into());
    }
    let measured_native_copies = native.issued() - native_baseline;
    let measured_copies = consumer.copies - copy_baseline;
    let measured_native_calls = consumer.native_calls_ns.len();
    Ok(serde_json::json!({
        "schema_version": 1, "fixture_version": 1, "fixture": "openvr-native-output",
        "status": if immutable {"bounded_deferred"} else {"active"},
        "evidence": "actual-openvr-gpu", "platform": "windows", "backend": "dx12-d3d11",
        "adapter": gpu.capabilities().adapter_name(), "device_generation": gpu.generation().get(),
        "extent": EXTENT, "compositor": "steamvr", "interface": "IVROverlay_028",
        "alpha_encoding": "gamma-premultiplied", "overlay_premultiplied": true,
        // This fixed opaque workload does not validate transparent edge pixels.
        "workload_alpha": "opaque",
        "mode": if immutable {"immutable_snapshots"} else {"shared_atomic"},
        "warmup_frames": warmup_frames, "warmup_attempts": warmup_attempts,
        "total_consumer_updates": consumer.updates_completed,
        "total_attempts": total_attempts, "attempts": attempts, "requested_frames": ATTEMPTS,
        "frames_received": produced, "submitted_to_compositor": measured_native_calls,
        "consumer_updates_completed": consumer.updates_completed - completed_baseline,
        "deferred": deferred, "consumer_slots": if immutable {CAPACITY} else {1},
        // Each steady update reuses the identity initialized in warmup.
        "consumer_slot_reuses": if immutable {0} else {measured_copies},
        "consumer_target_allocations": consumer.target_allocations - allocation_baseline,
        "consumer_resource_retirements": 0,
        "producer_ui_renders": producer_ui_renders, "producer_ui_submissions": producer_ui_submissions,
        "producer_draw_calls": producer_draw_calls, "producer_native_submissions": measured_native_copies,
        "producer_native_copies": measured_native_copies,
        "producer_native_copy_bytes": measured_native_copies * EXTENT[0] as u64 * EXTENT[1] as u64 * 4,
        "consumer_gpu_copies": measured_copies, "consumer_gpu_copy_bytes": consumer.copy_bytes - bytes_baseline,
        "cpu_readbacks": 0, "explicit_cpu_fence_waits": 0,
        "compositor_retirement_proven": false, "continuous_reuse_supported": !immutable,
        "resource_lifetime": "consumer-owned persistent identity held until process exit; local copy completion releases NanaUI token only",
        "atomic_update_contract": "Valve pinned openvr.h ETextureType::TextureType_DXGISharedHandle permits atomic CopyResource/resolve updates",
        "static": {"attempts": ATTEMPTS, "producer_ui_renders": static_ui_renders,
            "producer_submissions": static_submissions, "producer_native_copies": native.issued()-native_before,
            "consumer_gpu_copies": consumer.copies-static_before},
        "producer_record_cpu_ms": timing(&producer_calls_ns),
        "producer_native_record_cpu_ms": timing(&native_record_calls_ns),
        "consumer_record_cpu_ms": timing(&consumer.copy_calls_ns),
        "native_call_cpu_ms": timing(&consumer.native_calls_ns),
        "setup_scope": {"submitted_to_compositor": setup_native_calls_ns.len(),
            "native_call_cpu_ms": timing(&setup_native_calls_ns),
            "consumer_target_allocations": allocation_baseline},
        "timing_scope": if immutable {
            "Bounded immutable fixture has no warmup; CPU recording includes initialization. No continuous performance pass or GPU execution time is claimed"
        } else {
            "30 actual native updates prewarm shared_atomic; CPU recording quantiles cover only the following 240 updates. The first SetOverlayTexture call is separately observed in setup_scope; other one-time runtime setup is outside these timings. No GPU execution time or claim about runtime-internal blocking"
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_missing_interface_is_an_unsupported_capability() {
        assert!(matches!(
            interface_failure(105, true),
            ProbeError::Unsupported(_)
        ));
        for error in [0, 1, 200, 301] {
            assert!(matches!(
                interface_failure(error, true),
                ProbeError::Failed(_)
            ));
        }
    }

    #[test]
    fn official_overlay_028_table_offsets_are_exact() {
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(std::mem::offset_of!(OverlayTable, create), pointer);
        assert_eq!(std::mem::offset_of!(OverlayTable, destroy), 3 * pointer);
        assert_eq!(std::mem::offset_of!(OverlayTable, set_flag), 11 * pointer);
        assert_eq!(std::mem::offset_of!(OverlayTable, width), 22 * pointer);
        assert_eq!(std::mem::offset_of!(OverlayTable, transform), 35 * pointer);
        assert_eq!(std::mem::offset_of!(OverlayTable, show), 43 * pointer);
        assert_eq!(
            std::mem::offset_of!(OverlayTable, set_texture),
            60 * pointer
        );
        assert_eq!(std::mem::offset_of!(OverlayTable, clear), 61 * pointer);
    }

    #[test]
    fn premultiplied_configuration_uses_the_official_flag_and_propagates_runtime_errors() {
        unsafe extern "system" fn accepts_premultiplied(
            overlay: u64,
            flag: i32,
            enabled: bool,
        ) -> i32 {
            if overlay == 7 && flag == 2_097_152 && enabled {
                0
            } else {
                20 // VROverlayError_InvalidParameter
            }
        }
        unsafe extern "system" fn runtime_error(_: u64, _: i32, _: bool) -> i32 {
            23 // VROverlayError_RequestFailed
        }
        assert!(configure_premultiplied(accepts_premultiplied, 7).is_ok());
        let error = configure_premultiplied(runtime_error, 7).unwrap_err();
        assert_eq!(error, "OpenVR SetOverlayFlag(IsPremultiplied): error 23");
        assert!(matches!(ProbeError::from(error), ProbeError::Failed(_)));
    }

    #[test]
    fn loading_requires_an_explicit_existing_absolute_sdk_path() {
        assert!(OpenVr::load(std::path::Path::new("openvr_api.dll")).is_err());
        assert!(
            OpenVr::load(std::path::Path::new(
                "Z:\\missing-openvr-sdk\\openvr_api.dll"
            ))
            .is_err()
        );
    }

    #[test]
    #[ignore = "requires Windows DX12 adapter, SteamVR compositor and NANA_OPENVR_DLL absolute SDK path"]
    fn live_steamvr_shared_resource_streams_and_static_source_stays_idle() {
        let report = run().expect("execute the concrete OpenVR native path");
        assert_eq!(report["mode"], "shared_atomic");
        assert_eq!(report["alpha_encoding"], "gamma-premultiplied");
        assert_eq!(report["overlay_premultiplied"], true);
        assert_eq!(report["workload_alpha"], "opaque");
        assert_eq!(report["frames_received"], ATTEMPTS);
        assert_eq!(report["producer_native_copies"], ATTEMPTS);
        assert_eq!(report["consumer_gpu_copies"], ATTEMPTS);
        assert_eq!(report["consumer_updates_completed"], ATTEMPTS);
        assert_eq!(report["warmup_frames"], WARMUP_FRAMES);
        assert_eq!(report["total_consumer_updates"], ATTEMPTS + WARMUP_FRAMES);
        assert_eq!(report["submitted_to_compositor"], 0);
        assert_eq!(report["setup_scope"]["submitted_to_compositor"], 1);
        assert_eq!(report["consumer_target_allocations"], 0);
        assert_eq!(report["consumer_slot_reuses"], ATTEMPTS);
        assert_eq!(report["consumer_slots"], 1);
        assert_eq!(report["cpu_readbacks"], 0);
        assert_eq!(report["static"]["producer_ui_renders"], 0);
        assert_eq!(report["static"]["producer_submissions"], 0);
        assert_eq!(report["static"]["producer_native_copies"], 0);
        assert_eq!(report["static"]["consumer_gpu_copies"], 0);
    }
}
