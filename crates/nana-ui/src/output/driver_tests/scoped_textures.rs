//! Shared registries must invalidate only the presentations which reference a slot.
use super::*;
use crate::{HostTexture, HostTextureAlphaMode, HostTextureRegistry, NanaTextShaper};
use nana_ui_core::LengthSpec;
use nana_ui_runtime::{GpuTextureView, LayoutViewport};
use std::sync::Arc;

fn textured_scene(slot: &str) -> Arc<UiScene> {
    let id = DocumentId::new(1).unwrap();
    let mut document = RuntimeDocument::new(id);
    let mut view = GpuTextureView::new(slot);
    let layout = Arc::make_mut(&mut view.style.layout);
    layout.width = Some(LengthSpec::Px(EXTENT[0] as f32));
    layout.height = Some(LengthSpec::Px(EXTENT[1] as f32));
    document.context_mut().create_component(id, view).unwrap();
    document
        .flush(
            LayoutViewport::new(64.0, 48.0),
            &mut NanaTextShaper::default(),
        )
        .unwrap();
    document.shared_scene()
}

fn texture() -> HostTexture {
    let texture = test_gpu::context()
        .create_texture(&GpuTextureDescriptor {
            label: Some("scoped retained texture invalidation"),
            width: EXTENT[0],
            height: EXTENT[1],
            format: GpuTextureFormat::RGBA8_UNORM_SRGB,
            usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::RENDER_TARGET,
        })
        .unwrap();
    HostTexture::new(&texture)
}

fn draw(
    driver: &mut PresentationDriver,
    scene: &UiScene,
    registry: &HostTextureRegistry,
) -> ExternalRenderOutcome {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcome = driver
            .render_scene(scene, [64.0, 48.0], 0, CLEAR, Some(registry), None)
            .unwrap()[0];
        if !matches!(outcome, ExternalRenderOutcome::Deferred) {
            return outcome;
        }
        assert!(
            Instant::now() < deadline,
            "shared GPU did not make progress"
        );
        driver.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn unrelated_child_updates_leave_an_independent_static_parent_idle() {
    let registry = HostTextureRegistry::new();
    for slot in ["a", "b"] {
        registry.register(slot, texture(), 64, 48, HostTextureAlphaMode::Premultiplied);
    }
    let a_scene = textured_scene("a");
    let b_scene = textured_scene("b");
    let mut a = PresentationDriver::new(&test_gpu::context());
    let mut b = PresentationDriver::new(&test_gpu::context());
    a.configure(&[consumer(1)]).unwrap();
    b.configure(&[consumer(1)]).unwrap();
    assert!(matches!(
        draw(&mut a, &a_scene, &registry),
        ExternalRenderOutcome::Submitted { .. }
    ));
    assert!(matches!(
        draw(&mut b, &b_scene, &registry),
        ExternalRenderOutcome::Submitted { .. }
    ));
    drop(completed(&mut a, OutputConsumerId(1)));
    drop(completed(&mut b, OutputConsumerId(1)));
    registry.invalidate("a").unwrap();
    assert!(matches!(
        draw(&mut a, &a_scene, &registry),
        ExternalRenderOutcome::Submitted { .. }
    ));
    assert!(matches!(
        draw(&mut b, &b_scene, &registry),
        ExternalRenderOutcome::Reused { .. }
    ));
    assert_eq!(b.work().output_ui_renders, Some(0));
    assert_eq!(b.work().output_submissions, Some(0));
    assert_eq!(b.work().output_gpu_copies, Some(0));
    assert_eq!(b.work().output_target_recreates, Some(0));
    registry.remove("a").unwrap();
    assert!(matches!(
        draw(&mut b, &b_scene, &registry),
        ExternalRenderOutcome::Reused { .. }
    ));
}

#[test]
fn direct_host_texture_version_changes_invalidate_a_referenced_binding() {
    let registry = HostTextureRegistry::new();
    let resource = texture();
    registry.register(
        "a",
        resource.clone(),
        64,
        48,
        HostTextureAlphaMode::Premultiplied,
    );
    let scene = textured_scene("a");
    let mut driver = PresentationDriver::new(&test_gpu::context());
    driver.configure(&[consumer(1)]).unwrap();
    assert!(matches!(
        draw(&mut driver, &scene, &registry),
        ExternalRenderOutcome::Submitted { .. }
    ));
    drop(completed(&mut driver, OutputConsumerId(1)));
    let registry_revision = registry.revision();
    resource.invalidate();
    assert_eq!(registry.revision(), registry_revision);
    assert!(matches!(
        draw(&mut driver, &scene, &registry),
        ExternalRenderOutcome::Submitted { .. }
    ));
}

#[test]
fn prepared_binding_identity_detects_insert_remove_and_replacement() {
    use crate::{
        ScenePaintViewport,
        output::{ExternalPrepare, ExternalSurface, ExternalSurfaceConfig},
    };
    let gpu = test_gpu::context();
    let registry = HostTextureRegistry::new();
    let scene = textured_scene("a");
    let mut surface = ExternalSurface::new(
        &gpu,
        ExternalSurfaceConfig::new(EXTENT, GpuTextureFormat::RGBA8_UNORM_SRGB),
    )
    .unwrap();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 48.0],
        physical_size: EXTENT,
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: CLEAR,
        clear: true,
    };
    let mut frame = gpu.begin_frame("scoped binding mutation rejection");
    for mutation in 0..4 {
        let ExternalPrepare::Record(plan) = surface
            .prepare(&scene, mutation, viewport, Some(&registry))
            .unwrap()
        else {
            panic!("unused pool has a free target");
        };
        match mutation {
            0 | 1 => {
                registry.register("a", texture(), 64, 48, HostTextureAlphaMode::Premultiplied);
            }
            2 => {
                registry.remove("a").unwrap();
            }
            _ => {
                registry.register("a", texture(), 64, 48, HostTextureAlphaMode::Opaque);
            }
        }
        assert_eq!(
            surface.record(
                &plan,
                &mut frame,
                None,
                &scene,
                viewport,
                Some(&registry),
                None
            ),
            Err(ExternalSurfaceError::StalePlan)
        );
    }
    frame.discard();
}

#[test]
fn unrelated_replacement_between_prepare_and_record_keeps_the_plan_valid() {
    use crate::{
        ScenePaintViewport,
        output::{ExternalPrepare, ExternalSurface, ExternalSurfaceConfig},
    };
    let gpu = test_gpu::context();
    let registry = HostTextureRegistry::new();
    registry.register("a", texture(), 64, 48, HostTextureAlphaMode::Premultiplied);
    let scene = textured_scene("a");
    let mut surface = ExternalSurface::new(
        &gpu,
        ExternalSurfaceConfig::new(EXTENT, GpuTextureFormat::RGBA8_UNORM_SRGB),
    )
    .unwrap();
    let viewport = ScenePaintViewport {
        logical_size: [64.0, 48.0],
        physical_size: EXTENT,
        scale_factor: 1.0,
        scene_origin: [0.0; 2],
        target_origin: [0.0; 2],
        clear_color: CLEAR,
        clear: true,
    };
    let ExternalPrepare::Record(plan) = surface
        .prepare(&scene, 0, viewport, Some(&registry))
        .unwrap()
    else {
        panic!("unused pool has a free target");
    };
    registry.register(
        "unrelated",
        texture(),
        64,
        48,
        HostTextureAlphaMode::Premultiplied,
    );
    let mut frame = gpu.begin_frame("unrelated registry changes preserve a reservation");
    surface
        .record(
            &plan,
            &mut frame,
            None,
            &scene,
            viewport,
            Some(&registry),
            None,
        )
        .unwrap();
    frame.discard();
}
