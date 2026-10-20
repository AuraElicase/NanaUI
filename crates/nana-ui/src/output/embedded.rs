//! Retained same-device embedding through the ordinary HostTexture painter.
//!
//! A display lease survives any number of parent frames. Each recording also
//! pins its exact revision until GPU completion; dropping a node or replacing
//! a registry slot cannot return storage that an in-flight parent still uses.

use std::fmt;
use std::sync::{Arc, Mutex};

use nana_gpu::{DeviceGeneration, FrameContext, GpuContext, GpuError, GpuSubmission, GpuTexture};
use nana_ui_platform::EndpointGeneration;
use nana_ui_runtime::{DocumentId, StableNodeId};

use super::{ExternalFrame, ExternalSurfaceError, OutputSurfaceId};
use crate::{
    AlphaEncoding, HostTexture, HostTextureAlphaMode, HostTextureBinding, HostTextureRegistry,
    ScenePresentationColorSpace, ScenePresentationProfile,
};

/// Handle to the parent's authoritative presentation/input geometry.
///
/// This is a revisioned anchor, not another transform tree. Input integration
/// resolves `node` through `UiWorld::pointer_layout_position` and then uses
/// `GpuTextureView::texture_uv_at` for fit/zoom. The router remains responsible
/// for hit testing, clipping, focus and capture (Issue #243).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedInputMapping {
    pub document: DocumentId,
    pub node: StableNodeId,
    pub revision: u64,
    pub clip_revision: u64,
    pub visible: bool,
}

/// Output metadata travels with the completed child revision. Parent geometry
/// changes update the mapping independently and never repaint the child.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmbeddedOutputMetadata {
    source: Option<OutputSurfaceId>,
    logical_extent: [f32; 2],
    texture_extent: [u32; 2],
    resource_generation: u64,
    content_revision: u64,
    device_generation: DeviceGeneration,
    presentation: ScenePresentationProfile,
    alpha_encoding: AlphaEncoding,
    endpoint_generation: EndpointGeneration,
    input_mapping: Option<EmbeddedInputMapping>,
}

impl EmbeddedOutputMetadata {
    /// Build sRGB, premultiplied metadata. Use `from_frame` when a retained
    /// frame is available so its actual colour/alpha profile is preserved.
    pub fn new(
        logical_extent: [f32; 2],
        texture_extent: [u32; 2],
        resource_generation: u64,
        content_revision: u64,
        device_generation: DeviceGeneration,
    ) -> Result<Self, EmbeddedMetadataError> {
        validate_extents(logical_extent, texture_extent)?;
        Ok(Self {
            source: None,
            logical_extent,
            texture_extent,
            resource_generation,
            content_revision,
            device_generation,
            presentation: ScenePresentationProfile::sdr(
                nana_gpu::GpuTextureFormat::RGBA8_UNORM_SRGB,
            ),
            alpha_encoding: AlphaEncoding::Linear,
            endpoint_generation: EndpointGeneration(resource_generation),
            input_mapping: None,
        })
    }

    pub fn from_frame(
        frame: &ExternalFrame,
        logical_extent: [f32; 2],
    ) -> Result<Self, EmbeddedMetadataError> {
        let (width, height) = frame
            .texture()
            .map_err(|_| EmbeddedMetadataError::StaleFrame)?
            .size();
        let mut metadata = Self::new(
            logical_extent,
            [width, height],
            frame.resource_generation(),
            frame.content_revision(),
            frame.device_generation(),
        )?;
        metadata.presentation = frame.presentation();
        metadata.source = Some(frame.provenance().source());
        metadata.alpha_encoding = frame.alpha_encoding();
        Ok(metadata)
    }

    pub const fn logical_extent(self) -> [f32; 2] {
        self.logical_extent
    }
    pub const fn source(self) -> Option<OutputSurfaceId> {
        self.source
    }
    pub const fn texture_extent(self) -> [u32; 2] {
        self.texture_extent
    }
    pub const fn resource_generation(self) -> u64 {
        self.resource_generation
    }
    pub const fn content_revision(self) -> u64 {
        self.content_revision
    }
    pub const fn device_generation(self) -> DeviceGeneration {
        self.device_generation
    }
    pub const fn presentation(self) -> ScenePresentationProfile {
        self.presentation
    }
    pub const fn alpha_encoding(self) -> AlphaEncoding {
        self.alpha_encoding
    }
    pub const fn endpoint_generation(self) -> EndpointGeneration {
        self.endpoint_generation
    }
    pub const fn input_mapping(self) -> Option<EmbeddedInputMapping> {
        self.input_mapping
    }

    pub const fn with_endpoint_generation(mut self, generation: EndpointGeneration) -> Self {
        self.endpoint_generation = generation;
        self
    }

    pub const fn with_input_mapping(mut self, mapping: Option<EmbeddedInputMapping>) -> Self {
        self.input_mapping = mapping;
        self
    }

    fn supports_direct_sampling(self) -> bool {
        self.alpha_encoding == AlphaEncoding::Linear
            && match self.presentation.color_space {
                ScenePresentationColorSpace::Srgb => self.presentation.target_format.is_srgb(),
                ScenePresentationColorSpace::ExtendedSrgbLinear => {
                    !self.presentation.target_format.is_srgb()
                }
                _ => false,
            }
    }
}

fn validate_extents(logical: [f32; 2], texture: [u32; 2]) -> Result<(), EmbeddedMetadataError> {
    if !logical
        .iter()
        .all(|value| value.is_finite() && *value > 0.0)
    {
        return Err(EmbeddedMetadataError::InvalidLogicalExtent);
    }
    if texture.contains(&0) {
        return Err(EmbeddedMetadataError::InvalidTextureExtent);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedMetadataError {
    InvalidLogicalExtent,
    InvalidTextureExtent,
    StaleFrame,
    /// The target needs an explicit colour/alpha conversion before its pixels
    /// can be sampled into the parent's premultiplied linear-scRGB scene.
    UnsupportedSampling,
}

impl fmt::Display for EmbeddedMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidLogicalExtent => "embedded logical extent must be finite and positive",
            Self::InvalidTextureExtent => "embedded texture extent must be non-zero",
            Self::StaleFrame => "embedded frame is no longer current",
            Self::UnsupportedSampling => "embedded frame requires explicit colour/alpha conversion",
        })
    }
}
impl std::error::Error for EmbeddedMetadataError {}

/// A stable parent HostTexture binding backed by a retained child display
/// lease. Registry clones independently keep that revision from being reused.
/// Updating parent transform/clip/opacity only changes the parent scene.
#[derive(Debug)]
pub struct EmbeddedSurfaceNode {
    texture: GpuTexture,
    host_texture: HostTexture,
    metadata: EmbeddedOutputMetadata,
    frame: Mutex<Option<ExternalFrame>>,
    registrations: Mutex<Vec<EmbeddedRegistration>>,
}

#[derive(Debug)]
struct EmbeddedRegistration {
    registry: HostTextureRegistry,
    slot: String,
    alpha: HostTextureAlphaMode,
}

impl EmbeddedSurfaceNode {
    pub fn from_frame(
        frame: ExternalFrame,
        logical_extent: [f32; 2],
    ) -> Result<Self, EmbeddedMetadataError> {
        let metadata = EmbeddedOutputMetadata::from_frame(&frame, logical_extent)?;
        if !metadata.supports_direct_sampling() {
            return Err(EmbeddedMetadataError::UnsupportedSampling);
        }
        let texture = frame
            .texture()
            .map_err(|_| EmbeddedMetadataError::StaleFrame)?
            .clone();
        let host_texture = HostTexture::new_retained(
            &texture,
            Arc::new(frame.clone()),
            frame.provenance().clone(),
        );
        Ok(Self {
            texture,
            host_texture,
            metadata,
            frame: Mutex::new(Some(frame)),
            registrations: Mutex::new(Vec::new()),
        })
    }

    /// Replace the completed child revision without changing node, slot or
    /// HostTexture identity. Repeating the same revision is a no-op. Older
    /// revisions from the same producer generation are rejected as stale;
    /// delayed completion delivery cannot roll a displayed child backwards.
    /// Existing registrations publish the replacement and wake their consumers.
    pub fn update_frame(
        &mut self,
        frame: ExternalFrame,
        logical_extent: [f32; 2],
    ) -> Result<bool, EmbeddedMetadataError> {
        let mut metadata = EmbeddedOutputMetadata::from_frame(&frame, logical_extent)?;
        if !metadata.supports_direct_sampling() {
            return Err(EmbeddedMetadataError::UnsupportedSampling);
        }
        if metadata.source == self.metadata.source
            && metadata.resource_generation == self.metadata.resource_generation
            && metadata.content_revision < self.metadata.content_revision
        {
            return Err(EmbeddedMetadataError::StaleFrame);
        }
        metadata.endpoint_generation = self.metadata.endpoint_generation;
        metadata.input_mapping = self.metadata.input_mapping;
        if metadata == self.metadata
            && frame
                .texture()
                .is_ok_and(|texture| texture.ptr_eq(&self.texture))
            && self.frame.get_mut().is_ok_and(|current| current.is_some())
        {
            return Ok(false);
        }
        let texture = frame
            .texture()
            .map_err(|_| EmbeddedMetadataError::StaleFrame)?
            .clone();
        self.host_texture.replace_retained_texture(
            &texture,
            Arc::new(frame.clone()),
            frame.provenance().clone(),
        );
        self.texture = texture;
        self.metadata = metadata;
        *self
            .frame
            .get_mut()
            .map_err(|_| EmbeddedMetadataError::StaleFrame)? = Some(frame);
        let registrations = self
            .registrations
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        registrations.retain(|registration| {
            registration.registry.register_if_instance(
                &registration.slot,
                self.host_texture.clone(),
                self.metadata.texture_extent[0],
                self.metadata.texture_extent[1],
                registration.alpha,
            )
        });
        Ok(true)
    }

    /// Update only the output/input integration seam. The mapping points back
    /// to Runtime geometry; no transforms or routing state are duplicated.
    pub fn set_input_mapping(
        &mut self,
        endpoint: EndpointGeneration,
        mapping: Option<EmbeddedInputMapping>,
    ) {
        self.metadata.endpoint_generation = endpoint;
        self.metadata.input_mapping = mapping;
    }

    pub const fn metadata(&self) -> EmbeddedOutputMetadata {
        self.metadata
    }
    pub const fn resource_generation(&self) -> u64 {
        self.metadata.resource_generation
    }
    pub const fn content_revision(&self) -> u64 {
        self.metadata.content_revision
    }

    pub fn validate(&self) -> Result<(), ExternalSurfaceError> {
        self.frame
            .lock()
            .map_err(|_| ExternalSurfaceError::StaleFrame)?
            .as_ref()
            .ok_or(ExternalSurfaceError::StaleFrame)?
            .validate()
    }

    pub fn texture(&self) -> Result<&GpuTexture, ExternalSurfaceError> {
        self.validate()?;
        Ok(&self.texture)
    }

    pub fn host_texture(&self) -> &HostTexture {
        &self.host_texture
    }

    /// The registry owns a display lease through the HostTexture. Keeping the
    /// binding after this node is dropped is safe: the child cannot reuse that
    /// storage until the binding and all recorded GPU references are gone.
    pub fn register_host_texture(
        &self,
        registry: &HostTextureRegistry,
        slot: impl Into<String>,
        alpha_mode: HostTextureAlphaMode,
    ) -> HostTextureBinding {
        let slot = slot.into();
        let [width, height] = self.metadata.texture_extent;
        let binding =
            registry.register(&slot, self.host_texture.clone(), width, height, alpha_mode);
        let mut registrations = self
            .registrations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(registration) = registrations
            .iter_mut()
            .find(|entry| entry.slot == slot && entry.registry.same_registry(registry))
        {
            registration.alpha = alpha_mode;
        } else {
            registrations.push(EmbeddedRegistration {
                registry: registry.clone(),
                slot,
                alpha: alpha_mode,
            });
        }
        binding
    }

    pub fn is_current(&self) -> bool {
        self.validate().is_ok()
    }

    /// Stop explicit frame access and retention through this node. This
    /// compatibility operation leaves its HostTexture display references and
    /// attachments intact; ordinary HostTexture sampling can continue. Use
    /// [`Self::detach`] to revoke the tracked attachments and drop this node.
    pub fn retire(&self) -> bool {
        self.frame
            .lock()
            .is_ok_and(|mut frame| frame.take().is_some())
    }

    /// Revoke the attachments registered through this node and consume its
    /// own display references. Slots replaced by another owner are untouched.
    /// Returns the number of attachments actually removed.
    ///
    /// Independently cloned HostTexture bindings, frozen painter snapshots,
    /// and submitted parent frames retain their exact leases until their own
    /// retirement. Ordinary Drop continues to permit a registry to display
    /// the child; only explicit detach withdraws the tracked attachments.
    pub fn detach(self) -> usize {
        let instance = self.host_texture.instance_identity();
        let registrations = self
            .registrations
            .into_inner()
            .unwrap_or_else(|error| error.into_inner());
        registrations
            .into_iter()
            .filter(|registration| {
                registration
                    .registry
                    .remove_if_instance(&registration.slot, instance)
                    .is_some()
            })
            .count()
    }

    /// Pin this revision while recording; discard releases it immediately,
    /// submit releases it only when the parent GPU work completes. The ordinary
    /// HostTexture painter does this automatically for registered bindings.
    pub fn retain_in_frame(&self, frame: &mut FrameContext) -> Result<(), EmbeddedBindError> {
        let child = self.frame_clone()?;
        let expected = child.device_generation();
        let found = frame.generation();
        if expected != found {
            return Err(EmbeddedBindError::DeviceMismatch { expected, found });
        }
        frame.keep_alive_until_complete(child);
        Ok(())
    }

    fn frame_clone(&self) -> Result<ExternalFrame, EmbeddedBindError> {
        let frame = self
            .frame
            .lock()
            .map_err(|_| EmbeddedBindError::StaleFrame)?;
        let frame = frame.as_ref().ok_or(EmbeddedBindError::StaleFrame)?;
        frame
            .validate()
            .map_err(|_| EmbeddedBindError::StaleFrame)?;
        Ok(frame.clone())
    }

    /// Compatibility helper for hosts that bind lifetime after submission.
    /// Each call pins a separate reference: an active parent may sample the
    /// same completed child revision in any number of frames.
    pub fn bind_to_submission(
        &self,
        parent_gpu: &GpuContext,
        submission: &GpuSubmission,
    ) -> Result<EmbeddedFrameBinding, EmbeddedBindError> {
        let child = self.frame_clone()?;
        let expected = child.device_generation();
        for found in [parent_gpu.generation(), submission.generation()] {
            if expected != found {
                return Err(EmbeddedBindError::DeviceMismatch { expected, found });
            }
        }
        child
            .retire_after(parent_gpu, submission)
            .map_err(|error| match error {
                ExternalSurfaceError::Gpu(error) => EmbeddedBindError::Gpu(error),
                _ => EmbeddedBindError::StaleFrame,
            })?;
        Ok(EmbeddedFrameBinding {
            metadata: self.metadata,
            parent_generation: parent_gpu.generation(),
            parent_submission: submission.submission(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmbeddedFrameBinding {
    metadata: EmbeddedOutputMetadata,
    parent_generation: DeviceGeneration,
    parent_submission: u64,
}
impl EmbeddedFrameBinding {
    pub const fn metadata(self) -> EmbeddedOutputMetadata {
        self.metadata
    }
    pub const fn parent_generation(self) -> DeviceGeneration {
        self.parent_generation
    }
    pub const fn parent_submission(self) -> u64 {
        self.parent_submission
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedBindError {
    StaleFrame,
    DeviceMismatch {
        expected: DeviceGeneration,
        found: DeviceGeneration,
    },
    /// Retained for source compatibility; repeated bindings are now supported.
    AlreadyBound,
    Gpu(GpuError),
}
impl fmt::Display for EmbeddedBindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleFrame => {
                formatter.write_str("embedded child frame belongs to an old resource generation")
            }
            Self::DeviceMismatch { expected, found } => write!(
                formatter,
                "embedded child belongs to device {expected}, parent uses {found}"
            ),
            Self::AlreadyBound => {
                formatter.write_str("embedded child lease is already bound to a parent submission")
            }
            Self::Gpu(error) => error.fmt(formatter),
        }
    }
}
impl std::error::Error for EmbeddedBindError {}
impl From<GpuError> for EmbeddedBindError {
    fn from(error: GpuError) -> Self {
        Self::Gpu(error)
    }
}

#[cfg(test)]
mod tests;
