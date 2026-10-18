//! Common device resources for text and surface fragment adapters.
use crate::gpu_work::GpuWorkSink;
use nana_gpu::{LogicalBinding, LogicalBindingType, ResourceTable, ShaderInterface, ShaderStage};
use nana_ui_core::{MATERIAL_ABI, MaterialRef};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};

static DEVICES: LazyLock<Mutex<HashMap<nana_gpu::DeviceGeneration, Weak<MaterialDevice>>>> =
    LazyLock::new(Default::default);

pub(super) struct MaterialBinding {
    pub group: wgpu::BindGroup,
    _parameters: wgpu::Buffer,
    _textures: [Arc<MaterialImage>; 2],
}
struct MaterialImage {
    _source: Arc<[u8]>,
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
}
static WHITE: LazyLock<Arc<[u8]>> = LazyLock::new(|| Arc::from([255u8; 4]));
pub(super) struct MaterialDevice {
    interface: ShaderInterface,
    images: Mutex<HashMap<(usize, u32, u32), Arc<MaterialImage>>>,
    pub layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bindings: Mutex<HashMap<u64, Weak<MaterialBinding>>>,
    rejected: Mutex<HashMap<u64, Weak<nana_ui_core::MaterialInstance>>>,
    pipelines: Mutex<HashMap<(u64, wgpu::TextureFormat, u32, u64, u64), PipelineEntry>>,
}
struct PipelineEntry {
    owners: Vec<Weak<nana_ui_core::MaterialProgram>>,
    pipeline: Option<wgpu::RenderPipeline>,
}
impl MaterialDevice {
    pub(super) fn shared(
        device: &wgpu::Device,
        policy: Option<&nana_gpu::GpuDeviceState>,
    ) -> Arc<Self> {
        let mut devices = DEVICES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(found) = policy
            .and_then(|p| devices.get(&p.generation()))
            .and_then(Weak::upgrade)
        {
            return found;
        }
        let fragment = &[ShaderStage::Fragment];
        let table = ResourceTable::new(vec![
            LogicalBinding::new(
                0,
                LogicalBindingType::StorageBuffer { read_only: true },
                fragment,
            )
            .min_size(16),
            LogicalBinding::new(1, LogicalBindingType::SampledTexture, fragment),
            LogicalBinding::new(2, LogicalBindingType::SampledTexture, fragment),
            LogicalBinding::new(3, LogicalBindingType::Sampler, fragment),
        ])
        .expect("material resource table");
        // Use the same logical interface/layout key as other framework shaders.
        let interface = ShaderInterface::new(table, MATERIAL_ABI);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nana-ui.material.resources"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                texture_entry(1),
                texture_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let result = Arc::new(Self {
            interface,
            images: Mutex::new(HashMap::new()),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            bindings: Mutex::new(HashMap::new()),
            rejected: Mutex::new(HashMap::new()),
            pipelines: Mutex::new(HashMap::new()),
        });
        devices.retain(|_, value| value.strong_count() > 0);
        if let Some(policy) = policy {
            devices.insert(policy.generation(), Arc::downgrade(&result));
        }
        result
    }
    pub(super) fn pipeline(
        &self,
        key: (u64, wgpu::TextureFormat, u32, u64, u64),
        programs: &[&Arc<nana_ui_core::MaterialProgram>],
        device: &wgpu::Device,
        create: impl FnOnce() -> wgpu::RenderPipeline,
    ) -> Option<wgpu::RenderPipeline> {
        let key = (key.0 ^ self.interface.key(), key.1, key.2, key.3, key.4);
        let mut pipelines = self.pipelines.lock().unwrap_or_else(|e| e.into_inner());
        pipelines.retain(|_, entry| entry.owners.iter().all(|owner| owner.strong_count() > 0));
        pipelines
            .entry(key)
            .or_insert_with(|| PipelineEntry {
                owners: programs.iter().map(|p| Arc::downgrade(p)).collect(),
                pipeline: compile(device, create),
            })
            .pipeline
            .clone()
    }
    fn reject(&self, material: &MaterialRef) {
        let mut rejected = self.rejected.lock().unwrap_or_else(|e| e.into_inner());
        rejected.retain(|_, instance| instance.strong_count() > 0);
        if rejected
            .insert(material.key(), material.downgrade())
            .is_none()
        {
            nana_diagnostics::event!(&nana_diagnostics::framework::gpu::MATERIAL_REJECTED);
        }
    }
    pub(super) fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material: &MaterialRef,
        work: Option<&GpuWorkSink>,
    ) -> Option<Arc<MaterialBinding>> {
        let mut bindings = self.bindings.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(binding) = bindings.get(&material.key()).and_then(Weak::upgrade) {
            return Some(binding);
        }
        let params = material.parameters();
        if params.len() > 256
            || !params.iter().flatten().all(|v| v.is_finite())
            || material.textures().len() > 2
        {
            self.reject(material);
            return None;
        }
        for texture in material.textures() {
            let [w, h] = texture.size;
            let limit = device.limits().max_texture_dimension_2d.min(4096);
            if w == 0
                || h == 0
                || w > limit
                || h > limit
                || texture.rgba.len() as u64 != u64::from(w) * u64::from(h) * 4
            {
                self.reject(material);
                return None;
            }
        }
        let params = if params.is_empty() {
            &[[0.; 4]][..]
        } else {
            params
        };
        let parameters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("material parameters"),
            size: std::mem::size_of_val(params) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bytes = bytemuck::cast_slice(params);
        if let Some(work) = work {
            work.write_buffer(queue, &parameters, 0, bytes);
        } else {
            queue.write_buffer(&parameters, 0, bytes);
        }
        let mut images = self.images.lock().unwrap_or_else(|e| e.into_inner());
        images.retain(|_, image| Arc::strong_count(&image._source) > 1);
        let textures: [Arc<MaterialImage>; 2] = std::array::from_fn(|index| {
            let image = material.textures().get(index);
            let [w, h] = image.map_or([1, 1], |image| image.size);
            let rgba = image.map_or_else(|| Arc::clone(&WHITE), |image| image.rgba.clone());
            let key = (rgba.as_ptr() as usize, w, h);
            images
                .entry(key)
                .or_insert_with(|| {
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("material texture"),
                        size: wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    if let Some(work) = work {
                        work.write_texture(queue, &texture, [0, 0, 0], &rgba, w * 4, h, [w, h, 1]);
                    } else {
                        queue.write_texture(
                            texture.as_image_copy(),
                            &rgba,
                            wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(w * 4),
                                rows_per_image: Some(h),
                            },
                            texture.size(),
                        );
                    }
                    let view = texture.create_view(&Default::default());
                    Arc::new(MaterialImage {
                        _source: rgba,
                        _texture: texture,
                        view,
                    })
                })
                .clone()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material instance"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: parameters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&textures[0].view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&textures[1].view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        let binding = Arc::new(MaterialBinding {
            group,
            _parameters: parameters,
            _textures: textures,
        });
        bindings.retain(|_, value| value.strong_count() > 0);
        bindings.insert(material.key(), Arc::downgrade(&binding));
        Some(binding)
    }
}
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

/// Driver validation is scoped: an invalid extension falls back to native paint.
pub(super) fn compile(
    device: &wgpu::Device,
    create: impl FnOnce() -> wgpu::RenderPipeline,
) -> Option<wgpu::RenderPipeline> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = create();
    if [
        pollster::block_on(scope.pop()),
        pollster::block_on(internal.pop()),
        pollster::block_on(oom.pop()),
    ]
    .iter()
    .any(Option::is_some)
    {
        nana_diagnostics::event!(&nana_diagnostics::framework::gpu::MATERIAL_REJECTED);
        None
    } else {
        Some(result)
    }
}

pub(super) fn source(program: &nana_ui_core::MaterialProgram) -> Option<String> {
    program_source(program, "nm_text_", 2)
}

/// Namespace the WGSL IR, rather than replacing text: a helper named `color`
/// must never rename `input.color`, and comments/local names are irrelevant.
pub(super) fn program_source(
    program: &nana_ui_core::MaterialProgram,
    prefix: &str,
    group: u32,
) -> Option<String> {
    let result = (|| {
        if program.source.len() > 64 * 1024 {
            return None;
        }
        let mut module =
            naga::front::wgsl::parse_str(&format!("{}\n{}", program.source, MATERIAL_ABI)).ok()?;
        if !module.entry_points.is_empty()
            || module.global_variables.len() != 4
            || !module.overrides.is_empty()
        {
            return None;
        }
        let entry = module
            .functions
            .iter()
            .find(|(_, f)| f.name.as_deref() == Some("material"))?
            .1;
        if entry.arguments.len() != 1
            || module.types[entry.arguments[0].ty].name.as_deref() != Some("MaterialInput")
        {
            return None;
        }
        if !entry.result.as_ref().is_some_and(|result| {
            matches!(
                module.types[result.ty].inner,
                naga::TypeInner::Vector {
                    size: naga::VectorSize::Quad,
                    scalar: naga::Scalar {
                        kind: naga::ScalarKind::Float,
                        width: 4
                    }
                }
            )
        }) {
            return None;
        }
        // Rebuild in handle order: in-place replacement can collide with an
        // author's existing prefixed name before that type is renamed.
        let original = std::mem::take(&mut module.types);
        for (handle, ty) in original.iter() {
            let mut ty = ty.clone();
            if let Some(name) = &mut ty.name {
                name.insert_str(0, prefix);
            }
            if module.types.insert(ty, original.get_span(handle)) != handle {
                return None;
            }
        }
        for (_, constant) in module.constants.iter_mut() {
            if let Some(name) = &mut constant.name {
                name.insert_str(0, prefix);
            }
        }
        for (_, function) in module.functions.iter_mut() {
            if let Some(name) = &mut function.name {
                name.insert_str(0, prefix);
            }
        }
        for (_, variable) in module.global_variables.iter_mut() {
            if let Some(name) = &mut variable.name {
                name.insert_str(0, prefix);
            }
            if let Some(binding) = &mut variable.binding {
                if binding.group != 2 {
                    return None;
                }
                binding.group = group;
            }
        }
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .ok()?;
        naga::back::wgsl::write_string(&module, &info, naga::back::wgsl::WriterFlags::empty()).ok()
    })();
    if result.is_none() {
        nana_diagnostics::event!(&nana_diagnostics::framework::gpu::MATERIAL_REJECTED);
    }
    result
}
