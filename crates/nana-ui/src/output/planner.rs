//! Cached target topology for the consumers of a retained presentation.
//!
//! The host calls [`RenderTargetPlanner::update`] with resolved capabilities
//! at configuration boundaries. Recording a frame only reads [`Self::plan`](RenderTargetPlanner::plan):
//! it does not negotiate capabilities or rebuild a topology. A plan describes
//! work the host must actually execute; unsupported consumers never silently
//! select a CPU readback/upload path.

use nana_gpu::{DeviceGeneration, GpuTextureFormat};

use crate::ScenePresentationProfile;

/// Identity of a presentation, independent of any operating-system window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresentationId(pub u64);

/// Identity of an output consumer within one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputConsumerId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputConsumerKind {
    Window,
    External,
    Native,
}

/// Pixel alpha storage, which is part of compatibility, not widget state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAlpha {
    LinearPremultiplied,
    GammaPremultiplied,
    Straight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputTextAa {
    Grayscale,
    SubpixelRgb,
    SubpixelBgr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputOwnership {
    /// A bounded pool on the producer's own device.
    SameDevice,
    /// An explicitly supported native shared-resource boundary.
    NativeShared,
}

/// Resolved render requirements. These describe one rasterization, including
/// its viewport identity; differing text AA or pixel density cannot share a
/// render merely because their texture formats match.
///
/// Every consumer target is single-sample. Raster MSAA is internal to the
/// Scene painter, which resolves it before any consumer reads the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetRequirements {
    pub presentation: PresentationId,
    /// Changes when logical viewport, scene fitting, or pixel density changes.
    /// Content changes do not change this identity.
    pub viewport_revision: u64,
    pub extent: [u32; 2],
    pub profile: ScenePresentationProfile,
    pub alpha: OutputAlpha,
    pub text_aa: OutputTextAa,
    pub device_generation: DeviceGeneration,
    pub ownership: OutputOwnership,
    pub persistent: bool,
}

/// Capabilities of the concrete presenter/adapter installed by the host.
/// Setting a capability asserts an implemented path, not a backend wish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenterCapabilities {
    pub direct_render: bool,
    pub sampled: bool,
    pub copy_destination: bool,
    pub native_shared: bool,
    /// The installed adapter can render directly into an exportable canonical
    /// resource. Native import/export that requires a copy does not set this.
    pub native_render_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputConsumer {
    pub id: OutputConsumerId,
    pub kind: OutputConsumerKind,
    pub requirements: RenderTargetRequirements,
    pub capabilities: PresenterCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputUnsupportedReason {
    InvalidExtent,
    DuplicateConsumer,
    NoConsumerPath,
    NativeSharingUnavailable,
}

/// Lowest-cost implemented way this consumer reaches its group's target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputPath {
    DirectSurface,
    SharedTexture,
    NativeShared,
    GpuTransfer,
    Unsupported(OutputUnsupportedReason),
}

impl OutputPath {
    pub const fn is_supported(self) -> bool {
        !matches!(self, Self::Unsupported(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputConsumerBinding {
    pub consumer: OutputConsumerId,
    pub path: OutputPath,
    pub target_profile: ScenePresentationProfile,
    /// The canonical target and consumer use equivalent sRGB signals but
    /// different channel order or typed/untyped formats. The host must use its
    /// transfer shader, never a byte copy between those targets.
    pub requires_format_conversion: bool,
}

/// All supported bindings in a group consume one UI rasterization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputTargetGroup {
    pub requirements: RenderTargetRequirements,
    pub consumers: Vec<OutputConsumerBinding>,
    /// Zero for unsupported-only groups, one otherwise.
    pub render_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputIncompatibilityReason {
    Presentation,
    Viewport,
    Extent,
    Profile,
    Alpha,
    TextAa,
    DeviceGeneration,
    Ownership,
}

/// Why two consumers require distinct rasterizations. Persistence alone is
/// merged: a retained consumer can share with a transient one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputIncompatibility {
    pub first: OutputConsumerId,
    pub second: OutputConsumerId,
    pub reasons: Vec<OutputIncompatibilityReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutputPlan {
    pub groups: Vec<OutputTargetGroup>,
    pub incompatibilities: Vec<OutputIncompatibility>,
}

impl OutputPlan {
    pub fn binding(&self, consumer: OutputConsumerId) -> Option<&OutputConsumerBinding> {
        self.groups
            .iter()
            .flat_map(|group| &group.consumers)
            .find(|binding| binding.consumer == consumer)
    }

    pub fn render_count(&self) -> usize {
        self.groups.iter().map(|group| group.render_count).sum()
    }

    pub fn canonical_target_count(&self) -> usize {
        self.groups
            .iter()
            .filter(|group| {
                group.render_count != 0
                    && !group
                        .consumers
                        .iter()
                        .any(|binding| binding.path == OutputPath::DirectSurface)
            })
            .count()
    }
}

/// Cached, deterministic planning. The consumer slice has stable host order;
/// changing that order is a structural change. Content/frame revisions never
/// belong in this slice and therefore cannot cause per-frame negotiation.
#[derive(Debug, Default)]
pub struct RenderTargetPlanner {
    consumers: Vec<OutputConsumer>,
    plan: OutputPlan,
    rebuilds: u64,
}

impl RenderTargetPlanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether the topology changed. The unchanged path compares the
    /// existing slice and does not allocate or inspect capability combinations.
    pub fn update(&mut self, consumers: &[OutputConsumer]) -> bool {
        if self.consumers_match(consumers) {
            return false;
        }
        self.plan = build_plan(consumers);
        self.consumers.clear();
        self.consumers.extend_from_slice(consumers);
        self.rebuilds = self.rebuilds.saturating_add(1);
        true
    }

    pub fn plan(&self) -> &OutputPlan {
        &self.plan
    }

    /// Check a candidate without allocating or modifying the current plan.
    pub fn consumers_match(&self, consumers: &[OutputConsumer]) -> bool {
        self.consumers == consumers
    }

    pub const fn rebuild_count(&self) -> u64 {
        self.rebuilds
    }
}

fn build_plan(consumers: &[OutputConsumer]) -> OutputPlan {
    let mut plan = OutputPlan::default();
    for (index, consumer) in consumers.iter().enumerate() {
        let duplicate = consumers
            .iter()
            .filter(|other| other.id == consumer.id)
            .count()
            > 1;
        let path = if duplicate {
            OutputPath::Unsupported(OutputUnsupportedReason::DuplicateConsumer)
        } else {
            consumer_path(consumer, true)
        };
        // Explain all unequal consumer pairs, rather than making compatibility
        // appear dependent on whichever consumer happened to create a group.
        for previous in &consumers[..index] {
            let reasons = incompatibility(
                canonical_requirements(previous),
                canonical_requirements(consumer),
            );
            if !reasons.is_empty() {
                plan.incompatibilities.push(OutputIncompatibility {
                    first: previous.id,
                    second: consumer.id,
                    reasons,
                });
            }
        }
        let binding = OutputConsumerBinding {
            consumer: consumer.id,
            path,
            target_profile: consumer.requirements.profile,
            requires_format_conversion: false,
        };
        if let Some(group) = plan.groups.iter_mut().find(|group| {
            incompatibility(group.requirements, canonical_requirements(consumer)).is_empty()
        }) {
            if path.is_supported() && group.render_count == 0 {
                group.requirements = canonical_requirements(consumer);
            }
            if path.is_supported() {
                group.requirements.persistent |= consumer.requirements.persistent;
            }
            // An encoded UNORM canonical target can feed an external reader
            // directly and the Window through its sRGB format transfer.
            if path.is_supported()
                && consumer.requirements.profile.target_format == GpuTextureFormat::BGRA8_UNORM
            {
                group.requirements.profile = consumer.requirements.profile;
            }
            group.render_count |= usize::from(path.is_supported());
            group.consumers.push(binding);
        } else {
            plan.groups.push(OutputTargetGroup {
                requirements: canonical_requirements(consumer),
                consumers: vec![binding],
                render_count: usize::from(path.is_supported()),
            });
        }
    }
    for group in &mut plan.groups {
        // Independent presentations/windows keep their direct targets. Only
        // compatible, supported consumers of this group require sharing.
        let supported = group
            .consumers
            .iter()
            .filter(|binding| binding.path.is_supported())
            .count();
        if supported > 1 {
            for binding in &mut group.consumers {
                if binding.path == OutputPath::DirectSurface {
                    let consumer = consumers
                        .iter()
                        .find(|consumer| consumer.id == binding.consumer)
                        .expect("group consumer");
                    binding.path = consumer_path(consumer, false);
                }
            }
        } else if let Some(direct) = group
            .consumers
            .iter()
            .find(|binding| binding.path == OutputPath::DirectSurface)
        {
            let consumer = consumers
                .iter()
                .find(|consumer| consumer.id == direct.consumer)
                .expect("direct consumer");
            group.requirements = canonical_requirements(consumer);
        }
        group.render_count = usize::from(
            group
                .consumers
                .iter()
                .any(|binding| binding.path.is_supported()),
        );
        for binding in &mut group.consumers {
            binding.requires_format_conversion =
                binding.target_profile.target_format != group.requirements.profile.target_format;
        }
    }
    plan
}

fn canonical_requirements(consumer: &OutputConsumer) -> RenderTargetRequirements {
    let mut requirements = consumer.requirements;
    // A native export copy consumes the same canonical producer texture as a
    // same-device reader. Only a true renderable native canonical resource
    // needs a distinct ownership group.
    if requirements.ownership == OutputOwnership::NativeShared
        && consumer.capabilities.native_shared
        && consumer.capabilities.copy_destination
        && !consumer.capabilities.native_render_target
    {
        requirements.ownership = OutputOwnership::SameDevice;
    }
    requirements
}

fn consumer_path(consumer: &OutputConsumer, only_consumer: bool) -> OutputPath {
    use OutputUnsupportedReason as Unsupported;
    let requirements = consumer.requirements;
    if requirements.extent.contains(&0) {
        return OutputPath::Unsupported(Unsupported::InvalidExtent);
    }
    let capabilities = consumer.capabilities;
    if only_consumer
        && consumer.kind == OutputConsumerKind::Window
        && !requirements.persistent
        && requirements.ownership == OutputOwnership::SameDevice
        && capabilities.direct_render
    {
        return OutputPath::DirectSurface;
    }
    match requirements.ownership {
        OutputOwnership::NativeShared
            if capabilities.native_shared && capabilities.native_render_target =>
        {
            OutputPath::NativeShared
        }
        OutputOwnership::NativeShared
            if capabilities.native_shared && capabilities.copy_destination =>
        {
            OutputPath::GpuTransfer
        }
        OutputOwnership::NativeShared => {
            OutputPath::Unsupported(Unsupported::NativeSharingUnavailable)
        }
        OutputOwnership::SameDevice if consumer.kind == OutputConsumerKind::Window => {
            if capabilities.copy_destination {
                OutputPath::GpuTransfer
            } else {
                OutputPath::Unsupported(Unsupported::NoConsumerPath)
            }
        }
        OutputOwnership::SameDevice if capabilities.sampled => OutputPath::SharedTexture,
        OutputOwnership::SameDevice if capabilities.copy_destination => OutputPath::GpuTransfer,
        OutputOwnership::SameDevice => OutputPath::Unsupported(Unsupported::NoConsumerPath),
    }
}

fn incompatibility(
    first: RenderTargetRequirements,
    second: RenderTargetRequirements,
) -> Vec<OutputIncompatibilityReason> {
    use OutputIncompatibilityReason as Reason;
    let mut reasons = Vec::new();
    for (different, reason) in [
        (
            first.presentation != second.presentation,
            Reason::Presentation,
        ),
        (
            first.viewport_revision != second.viewport_revision,
            Reason::Viewport,
        ),
        (first.extent != second.extent, Reason::Extent),
        (
            !compatible_profiles(first.profile, second.profile),
            Reason::Profile,
        ),
        (first.alpha != second.alpha, Reason::Alpha),
        (first.text_aa != second.text_aa, Reason::TextAa),
        (
            first.device_generation != second.device_generation,
            Reason::DeviceGeneration,
        ),
        (first.ownership != second.ownership, Reason::Ownership),
    ] {
        if different {
            reasons.push(reason);
        }
    }
    reasons
}

fn compatible_profiles(first: ScenePresentationProfile, second: ScenePresentationProfile) -> bool {
    if first == second {
        return true;
    }
    // The final transfer samples logical RGBA channels, so the texture view
    // handles BGRA/RGBA channel order as well as typed sRGB conversion.
    let sdr_format = |format| {
        matches!(
            format,
            GpuTextureFormat::BGRA8_UNORM
                | GpuTextureFormat::BGRA8_UNORM_SRGB
                | GpuTextureFormat::RGBA8_UNORM
                | GpuTextureFormat::RGBA8_UNORM_SRGB
        )
    };
    sdr_format(first.target_format)
        && sdr_format(second.target_format)
        && first.color_space == crate::ScenePresentationColorSpace::Srgb
        && second.color_space == crate::ScenePresentationColorSpace::Srgb
        && first.force_float_working == second.force_float_working
        && first.is_direct_linear() == second.is_direct_linear()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_gpu::GpuTextureFormat;

    fn consumer(id: u64) -> OutputConsumer {
        OutputConsumer {
            id: OutputConsumerId(id),
            kind: OutputConsumerKind::External,
            requirements: RenderTargetRequirements {
                presentation: PresentationId(1),
                viewport_revision: 1,
                extent: [64, 48],
                profile: ScenePresentationProfile::sdr(GpuTextureFormat::RGBA8_UNORM_SRGB),
                alpha: OutputAlpha::LinearPremultiplied,
                text_aa: OutputTextAa::Grayscale,
                device_generation: crate::test_gpu::context().generation(),
                ownership: OutputOwnership::SameDevice,
                persistent: false,
            },
            capabilities: PresenterCapabilities {
                direct_render: true,
                sampled: true,
                copy_destination: true,
                native_shared: false,
                native_render_target: false,
            },
        }
    }

    #[test]
    fn window_only_stays_direct_without_a_canonical_texture() {
        let mut window = consumer(1);
        window.kind = OutputConsumerKind::Window;
        let mut planner = RenderTargetPlanner::new();
        assert!(planner.update(&[window]));
        assert_eq!(
            planner.plan().groups[0].consumers[0].path,
            OutputPath::DirectSurface
        );
        assert_eq!(planner.plan().canonical_target_count(), 0);
        assert_eq!(planner.plan().render_count(), 1);
        let groups = planner.plan().groups.as_ptr();
        for _ in 0..240 {
            assert!(!planner.update(&[window]));
            assert_eq!(planner.plan().groups.as_ptr(), groups);
        }
        assert_eq!(planner.rebuild_count(), 1);
    }

    #[test]
    fn compatible_consumers_share_one_render_and_persistence_is_merged() {
        let mut window = consumer(1);
        window.kind = OutputConsumerKind::Window;
        let mut external = consumer(2);
        external.requirements.persistent = true;
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[window, external, consumer(3)]);
        assert_eq!(planner.plan().groups.len(), 1);
        assert_eq!(planner.plan().render_count(), 1);
        assert_eq!(planner.plan().canonical_target_count(), 1);
        assert!(planner.plan().groups[0].requirements.persistent);
        assert!(planner.plan().incompatibilities.is_empty());
        assert_eq!(
            planner.plan().binding(window.id).unwrap().path,
            OutputPath::GpuTransfer
        );
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::SharedTexture
        );
    }

    #[test]
    fn each_rasterization_difference_is_explicit() {
        let first = consumer(1);
        let mut second = consumer(2);
        second.requirements.extent = [128, 48];
        second.requirements.viewport_revision = 2;
        second.requirements.alpha = OutputAlpha::GammaPremultiplied;
        second.requirements.text_aa = OutputTextAa::SubpixelRgb;
        second.requirements.profile =
            ScenePresentationProfile::sdr(GpuTextureFormat::BGRA8_UNORM).with_force_float_working();
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[first, second]);
        assert_eq!(planner.plan().render_count(), 2);
        assert_eq!(
            planner.plan().incompatibilities[0].reasons,
            vec![
                OutputIncompatibilityReason::Viewport,
                OutputIncompatibilityReason::Extent,
                OutputIncompatibilityReason::Profile,
                OutputIncompatibilityReason::Alpha,
                OutputIncompatibilityReason::TextAa,
            ]
        );
    }

    #[test]
    fn typed_and_untyped_srgb_bgra_share_with_an_explicit_window_transfer() {
        let mut window = consumer(1);
        window.kind = OutputConsumerKind::Window;
        window.requirements.profile =
            ScenePresentationProfile::sdr(GpuTextureFormat::BGRA8_UNORM_SRGB);
        let mut external = consumer(2);
        external.requirements.profile =
            ScenePresentationProfile::sdr(GpuTextureFormat::BGRA8_UNORM);
        external.requirements.persistent = true;
        let mut planner = RenderTargetPlanner::new();
        for consumers in [[window, external], [external, window]] {
            planner.update(&consumers);
            assert_eq!(planner.plan().groups.len(), 1);
            assert_eq!(planner.plan().render_count(), 1);
            assert!(planner.plan().incompatibilities.is_empty());
            assert_eq!(
                planner.plan().groups[0].requirements.profile,
                external.requirements.profile
            );
            let binding = planner.plan().binding(window.id).unwrap();
            assert_eq!(binding.path, OutputPath::GpuTransfer);
            assert_eq!(binding.target_profile, window.requirements.profile);
            assert!(binding.requires_format_conversion);
            assert!(
                !planner
                    .plan()
                    .binding(external.id)
                    .unwrap()
                    .requires_format_conversion
            );
        }
        external.requirements.profile = external.requirements.profile.with_force_float_working();
        planner.update(&[window, external]);
        assert_eq!(planner.plan().groups.len(), 2);
        assert_eq!(
            planner.plan().incompatibilities[0].reasons,
            vec![OutputIncompatibilityReason::Profile]
        );
    }

    #[test]
    fn sdr_channel_order_and_srgb_view_pairs_share_but_linear_profiles_do_not() {
        let formats = [
            GpuTextureFormat::BGRA8_UNORM,
            GpuTextureFormat::BGRA8_UNORM_SRGB,
            GpuTextureFormat::RGBA8_UNORM,
            GpuTextureFormat::RGBA8_UNORM_SRGB,
        ];
        let mut planner = RenderTargetPlanner::new();
        for first in formats {
            for second in formats {
                let mut window = consumer(1);
                window.kind = OutputConsumerKind::Window;
                window.requirements.profile = ScenePresentationProfile::sdr(first);
                let mut external = consumer(2);
                external.requirements.profile = ScenePresentationProfile::sdr(second);
                for consumers in [[window, external], [external, window]] {
                    planner.update(&consumers);
                    assert_eq!(planner.plan().render_count(), 1);
                    assert!(planner.plan().incompatibilities.is_empty());
                    assert_eq!(
                        planner.plan().binding(window.id).unwrap().path,
                        OutputPath::GpuTransfer
                    );
                }
                external.requirements.profile =
                    ScenePresentationProfile::legacy_linear(GpuTextureFormat::RGBA8_UNORM);
                planner.update(&[window, external]);
                assert_eq!(planner.plan().render_count(), 2);
                assert_eq!(
                    planner.plan().incompatibilities[0].reasons,
                    vec![OutputIncompatibilityReason::Profile]
                );
            }
        }
    }

    #[test]
    fn invalid_extent_and_distinct_presentation_or_ownership_are_not_silently_merged() {
        let first = consumer(1);
        let mut second = consumer(2);
        second.requirements.presentation = PresentationId(2);
        second.requirements.ownership = OutputOwnership::NativeShared;
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[first, second]);
        assert_eq!(planner.plan().groups.len(), 2);
        assert_eq!(planner.plan().render_count(), 1);
        assert_eq!(
            planner.plan().incompatibilities[0].reasons,
            vec![
                OutputIncompatibilityReason::Presentation,
                OutputIncompatibilityReason::Ownership
            ]
        );
        second.requirements.extent = [0, 48];
        planner.update(&[second]);
        assert_eq!(
            planner.plan().binding(second.id).unwrap().path,
            OutputPath::Unsupported(OutputUnsupportedReason::InvalidExtent)
        );
        assert_eq!(planner.plan().canonical_target_count(), 0);
    }

    #[test]
    fn capability_changes_replan_and_never_choose_an_unimplemented_cpu_path() {
        let mut external = consumer(1);
        external.capabilities.sampled = false;
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[external]);
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::GpuTransfer
        );
        external.capabilities.copy_destination = false;
        assert!(planner.update(&[external]));
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::Unsupported(OutputUnsupportedReason::NoConsumerPath)
        );
        assert_eq!(planner.plan().render_count(), 0);
        external.requirements.ownership = OutputOwnership::NativeShared;
        planner.update(&[external]);
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::Unsupported(OutputUnsupportedReason::NativeSharingUnavailable)
        );
        external.capabilities.native_shared = true;
        planner.update(&[external]);
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::Unsupported(OutputUnsupportedReason::NativeSharingUnavailable)
        );
        external.capabilities.copy_destination = true;
        planner.update(&[external]);
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::GpuTransfer
        );
        external.capabilities.native_render_target = true;
        planner.update(&[external]);
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::NativeShared
        );
    }

    #[test]
    fn invalid_or_duplicate_consumers_do_not_request_rendering() {
        let mut external = consumer(1);
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[external, external]);
        assert_eq!(planner.plan().render_count(), 0);
        assert!(
            planner.plan().groups[0]
                .consumers
                .iter()
                .all(|binding| binding.path
                    == OutputPath::Unsupported(OutputUnsupportedReason::DuplicateConsumer))
        );
        external.requirements.extent = [0, 48];
        planner.update(&[external]);
        assert_eq!(
            planner.plan().binding(external.id).unwrap().path,
            OutputPath::Unsupported(OutputUnsupportedReason::InvalidExtent)
        );
        assert_eq!(planner.plan().render_count(), 0);
    }

    #[test]
    fn topology_removal_restores_direct_window_and_releases_groups() {
        let mut window = consumer(1);
        window.kind = OutputConsumerKind::Window;
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[window, consumer(2)]);
        assert_eq!(planner.plan().canonical_target_count(), 1);
        planner.update(&[window]);
        assert_eq!(
            planner.plan().binding(window.id).unwrap().path,
            OutputPath::DirectSurface
        );
        planner.update(&[]);
        assert!(planner.plan().groups.is_empty());
        assert_eq!(planner.plan().render_count(), 0);
        assert!(!planner.update(&[]));
    }

    #[test]
    fn independent_windows_and_an_unsupported_reader_do_not_force_offscreen_targets() {
        let mut first = consumer(1);
        first.kind = OutputConsumerKind::Window;
        let mut second = first;
        second.id = OutputConsumerId(2);
        second.requirements.presentation = PresentationId(2);
        let mut unsupported = consumer(3);
        unsupported.requirements.persistent = true;
        unsupported.capabilities = PresenterCapabilities::default();
        let mut planner = RenderTargetPlanner::new();
        planner.update(&[first, second, unsupported]);
        assert_eq!(planner.plan().render_count(), 2);
        assert_eq!(planner.plan().canonical_target_count(), 0);
        assert_eq!(
            planner.plan().binding(first.id).unwrap().path,
            OutputPath::DirectSurface
        );
        assert_eq!(
            planner.plan().binding(second.id).unwrap().path,
            OutputPath::DirectSurface
        );
        assert!(matches!(
            planner.plan().binding(unsupported.id).unwrap().path,
            OutputPath::Unsupported(_)
        ));
    }

    #[test]
    fn native_copy_export_consumes_the_same_canonical_render() {
        let external = consumer(1);
        let mut native = consumer(2);
        native.kind = OutputConsumerKind::Native;
        native.requirements.ownership = OutputOwnership::NativeShared;
        native.capabilities.native_shared = true;
        let mut planner = RenderTargetPlanner::new();
        for consumers in [[external, native], [native, external]] {
            planner.update(&consumers);
            assert_eq!(planner.plan().render_count(), 1);
            assert_eq!(planner.plan().groups.len(), 1);
            assert_eq!(
                planner.plan().groups[0].requirements.ownership,
                OutputOwnership::SameDevice
            );
            assert!(planner.plan().incompatibilities.is_empty());
            assert_eq!(
                planner.plan().binding(native.id).unwrap().path,
                OutputPath::GpuTransfer
            );
        }
        native.capabilities.native_render_target = true;
        planner.update(&[external, native]);
        assert_eq!(planner.plan().groups.len(), 2);
        assert_eq!(
            planner.plan().incompatibilities[0].reasons,
            vec![OutputIncompatibilityReason::Ownership]
        );
    }
}
