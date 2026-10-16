//! Windowless production uses the same host frame and real GPU submission as UI.
use super::*;
use crate::{
    HostTexture, HostTextureAlphaMode, HostTextureRegistry, NanaTextShaper,
    SceneResourceEncodeContext, SceneResourceProducer, SceneResourceProducerRegistry,
};
use nana_gpu::{__framework as raw, FrameId, GpuSubmission};
use nana_ui_core::LengthSpec;
use nana_ui_runtime::{CustomRenderNode, GpuTextureView, LayoutViewport};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Debug)]
struct Producer {
    texture: GpuTexture,
    version: AtomicU64,
    versioned: bool,
    fail: AtomicBool,
    encoded: Mutex<Vec<FrameId>>,
    submitted: Mutex<Vec<FrameId>>,
}

impl SceneResourceProducer for Producer {
    fn preparation_version(&self, _: &CustomRenderNode) -> Option<u64> {
        self.versioned.then(|| self.version.load(Ordering::Relaxed))
    }

    fn encode(
        &self,
        _: &CustomRenderNode,
        mut context: SceneResourceEncodeContext<'_>,
    ) -> Result<(), String> {
        let frame = context.frame();
        self.encoded.lock().unwrap().push(frame.id());
        let green = self.version.load(Ordering::Relaxed) > 1;
        let pass = raw::encoder(frame).begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("headless resource producer pixel fixture"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: raw::texture_view(&self.texture),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: if green { 0.0 } else { 1.0 },
                        g: if green { 1.0 } else { 0.0 },
                        b: 0.0,
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
        drop(pass);
        if self.fail.load(Ordering::Relaxed) {
            Err("intentional error after producer encoding".into())
        } else {
            Ok(())
        }
    }

    fn submitted(&self, _: &CustomRenderNode, submission: &GpuSubmission) {
        self.submitted.lock().unwrap().push(submission.frame());
    }
}

fn fixture(versioned: bool) -> (Arc<Producer>, HostTextureRegistry, Arc<UiScene>) {
    let texture = test_gpu::context()
        .create_texture(&GpuTextureDescriptor {
            label: Some("headless resource producer sampled target"),
            width: EXTENT[0],
            height: EXTENT[1],
            format: GpuTextureFormat::RGBA8_UNORM_SRGB,
            usage: GpuTextureUsages::SAMPLED
                | GpuTextureUsages::RENDER_TARGET
                | GpuTextureUsages::COPY_SRC,
        })
        .unwrap();
    let textures = HostTextureRegistry::new();
    textures.register(
        "source",
        HostTexture::new(1, 1, &texture),
        64,
        48,
        HostTextureAlphaMode::Premultiplied,
    );
    let producer = Arc::new(Producer {
        texture,
        version: AtomicU64::new(1),
        versioned,
        fail: AtomicBool::new(false),
        encoded: Mutex::new(Vec::new()),
        submitted: Mutex::new(Vec::new()),
    });
    let id = DocumentId::new(1).unwrap();
    let mut document = RuntimeDocument::new(id);
    let mut view = GpuTextureView::new("source");
    let layout = Arc::make_mut(&mut view.style.layout);
    layout.width = Some(LengthSpec::Px(64.0));
    layout.height = Some(LengthSpec::Px(48.0));
    document.context_mut().create_component(id, view).unwrap();
    document
        .flush(
            LayoutViewport::new(64.0, 48.0),
            &mut NanaTextShaper::default(),
        )
        .unwrap();
    (producer, textures, document.shared_scene())
}

fn registry(producer: &Arc<Producer>) -> Arc<SceneResourceProducerRegistry> {
    let mut registry = SceneResourceProducerRegistry::new();
    registry.insert("source", producer.clone());
    Arc::new(registry)
}

fn draw(
    driver: &mut PresentationDriver,
    scene: &UiScene,
    textures: &HostTextureRegistry,
) -> Vec<ExternalRenderOutcome> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcomes = driver
            .render_scene(scene, [64.0, 48.0], 0, CLEAR, Some(textures), None)
            .unwrap();
        if outcomes
            .iter()
            .all(|outcome| !matches!(outcome, ExternalRenderOutcome::Deferred))
        {
            return outcomes;
        }
        assert!(
            Instant::now() < deadline,
            "producer frame never became available"
        );
        driver.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn graph_producer_and_all_ui_groups_share_one_real_submission() {
    let (producer, textures, scene) = fixture(true);
    let gpu = test_gpu::context();
    let mut driver = PresentationDriver::new(&gpu);
    let mut second = consumer(2);
    second.requirements.extent = [80, 48];
    second.requirements.viewport_revision = 2;
    driver.configure(&[consumer(1), second]).unwrap();
    driver.set_resource_producers(Some(registry(&producer)));
    assert_eq!(draw(&mut driver, &scene, &textures).len(), 2);
    assert_eq!(driver.work().output_submissions, Some(1));
    assert_eq!(driver.work().output_ui_renders, Some(2));
    let encoded = producer.encoded.lock().unwrap().clone();
    assert_eq!(
        encoded.len(),
        1,
        "production must not repeat for each UI target"
    );
    assert_eq!(*producer.submitted.lock().unwrap(), encoded);
    let output = completed(&mut driver, OutputConsumerId(1));
    let pixels = read_pixels(output.texture().unwrap());
    let center = ((EXTENT[1] / 2 * EXTENT[0] + EXTENT[0] / 2) * 4) as usize;
    assert_eq!(&pixels[center..center + 4], &[0, 0, 255, 255]);
    drop(completed(&mut driver, OutputConsumerId(2)));
    assert!(
        draw(&mut driver, &scene, &textures)
            .iter()
            .all(|outcome| matches!(outcome, ExternalRenderOutcome::Reused { .. }))
    );
    assert_eq!(driver.work().output_submissions, Some(0));
    assert_eq!(producer.encoded.lock().unwrap().len(), 1);
    producer.version.store(2, Ordering::Relaxed);
    assert!(
        draw(&mut driver, &scene, &textures)
            .iter()
            .all(|outcome| matches!(outcome, ExternalRenderOutcome::Submitted { .. }))
    );
    assert_eq!(producer.encoded.lock().unwrap().len(), 2);
    assert_eq!(
        *producer.submitted.lock().unwrap(),
        *producer.encoded.lock().unwrap()
    );
    let latest = super::completed(&mut driver, OutputConsumerId(1));
    // Wait for the second revision rather than accepting the previous completed image.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut latest = latest;
    while latest.content_revision() == output.content_revision() {
        assert!(
            Instant::now() < deadline,
            "new producer pixels did not complete"
        );
        drop(latest);
        driver.poll();
        std::thread::sleep(Duration::from_millis(1));
        latest = completed(&mut driver, OutputConsumerId(1));
    }
    let pixels = read_pixels(latest.texture().unwrap());
    assert_eq!(&pixels[center..center + 4], &[0, 255, 0, 255]);
}

#[test]
fn unused_producers_do_not_turn_static_headless_output_volatile() {
    let (producer, textures, _) = fixture(false);
    let scene = UiScene::new();
    let mut driver = PresentationDriver::new(&test_gpu::context());
    driver.configure(&[consumer(1)]).unwrap();
    driver.set_resource_producers(Some(registry(&producer)));
    draw(&mut driver, &scene, &textures);
    drop(completed(&mut driver, OutputConsumerId(1)));
    assert!(matches!(
        draw(&mut driver, &scene, &textures)[0],
        ExternalRenderOutcome::Reused { .. }
    ));
    assert_eq!(driver.work().output_submissions, Some(0));
    assert!(producer.encoded.lock().unwrap().is_empty());
    assert!(producer.submitted.lock().unwrap().is_empty());
}

#[test]
fn unversioned_producers_preserve_dynamic_compatibility() {
    let (producer, textures, scene) = fixture(false);
    let mut driver = PresentationDriver::new(&test_gpu::context());
    driver.configure(&[consumer(1)]).unwrap();
    driver.set_resource_producers(Some(registry(&producer)));
    for expected in 1..=2 {
        assert!(matches!(
            draw(&mut driver, &scene, &textures)[0],
            ExternalRenderOutcome::Submitted { .. }
        ));
        drop(completed(&mut driver, OutputConsumerId(1)));
        assert_eq!(producer.encoded.lock().unwrap().len(), expected);
        assert_eq!(producer.submitted.lock().unwrap().len(), expected);
    }
}

#[test]
fn failed_production_discards_encoded_commands_and_never_notifies_submission() {
    let (producer, textures, scene) = fixture(true);
    let mut driver = PresentationDriver::new(&test_gpu::context());
    driver.configure(&[consumer(1)]).unwrap();
    driver.set_resource_producers(Some(registry(&producer)));
    draw(&mut driver, &scene, &textures);
    let before = completed(&mut driver, OutputConsumerId(1));
    let old_pixels = read_pixels(&producer.texture);
    producer.version.store(2, Ordering::Relaxed);
    producer.fail.store(true, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match driver.render_scene(&scene, [64.0, 48.0], 0, CLEAR, Some(&textures), None) {
            Err(_) => break,
            Ok(outcomes) => assert!(
                outcomes
                    .iter()
                    .all(|outcome| matches!(outcome, ExternalRenderOutcome::Deferred))
            ),
        }
        assert!(
            Instant::now() < deadline,
            "failing producer never acquired a frame"
        );
        driver.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(producer.submitted.lock().unwrap().len(), 1);
    assert_eq!(
        read_pixels(&producer.texture),
        old_pixels,
        "failed producer pass must not reach the GPU"
    );
    assert_eq!(
        driver
            .sample(OutputConsumerId(1))
            .unwrap()
            .content_revision(),
        before.content_revision()
    );
    producer.fail.store(false, Ordering::Relaxed);
    assert!(matches!(
        draw(&mut driver, &scene, &textures)[0],
        ExternalRenderOutcome::Submitted { .. }
    ));
    assert_eq!(producer.submitted.lock().unwrap().len(), 2);
}

#[test]
fn saturated_retained_slots_defer_resource_production_until_an_old_lease_retires() {
    let (producer, textures, scene) = fixture(true);
    let mut driver = PresentationDriver::new(&test_gpu::context());
    driver.configure(&[consumer(1)]).unwrap();
    driver.set_resource_producers(Some(registry(&producer)));
    let mut leases: Vec<ExternalFrame> = Vec::new();
    for version in 1..=3 {
        producer.version.store(version, Ordering::Relaxed);
        let ExternalRenderOutcome::Submitted {
            content_revision, ..
        } = draw(&mut driver, &scene, &textures)[0]
        else {
            panic!("changed producer must publish a new revision");
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let lease = loop {
            driver.poll();
            if let Ok(lease) = driver.sample(OutputConsumerId(1))
                && lease.content_revision() == content_revision
            {
                break lease;
            }
            assert!(
                Instant::now() < deadline,
                "changed producer did not complete"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(
            leases
                .iter()
                .all(|old| !old.texture().unwrap().ptr_eq(lease.texture().unwrap()))
        );
        leases.push(lease);
    }
    assert_eq!(producer.encoded.lock().unwrap().len(), 3);
    assert_eq!(producer.submitted.lock().unwrap().len(), 3);
    producer.version.store(4, Ordering::Relaxed);
    for _ in 0..8 {
        let outcomes = driver
            .render_scene(&scene, [64.0, 48.0], 0, CLEAR, Some(&textures), None)
            .unwrap();
        assert_eq!(outcomes, vec![ExternalRenderOutcome::Deferred]);
        assert_eq!(producer.encoded.lock().unwrap().len(), 3);
        assert_eq!(producer.submitted.lock().unwrap().len(), 3);
        assert_eq!(driver.work().output_submissions, Some(0));
        assert_eq!(driver.work().output_ui_renders, Some(0));
    }
    // The oldest lease is not the published revision; dropping it makes one
    // actual producer slot writable without withdrawing the displayed frame.
    drop(leases.remove(0));
    assert!(matches!(
        draw(&mut driver, &scene, &textures)[0],
        ExternalRenderOutcome::Submitted { .. }
    ));
    assert_eq!(producer.encoded.lock().unwrap().len(), 4);
    assert_eq!(producer.submitted.lock().unwrap().len(), 4);
    assert_eq!(driver.work().output_submissions, Some(1));
}
