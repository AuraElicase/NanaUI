//! Maintained real GPU transfer workloads with explicitly mock foreign reads.
//! They exercise adapter lifecycle, never SteamVR/Chromium conformance.

use super::*;
use nana_ui::output::{
    OverlayAdapterState, OverlayHostAdapter, OverlayRelease, OverlayTransport,
    OverlayTransportError,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub const WORKLOADS: [&str; 6] = [
    "overlay-static",
    "overlay-changing",
    "overlay-multi",
    "overlay-slow",
    "web-static",
    "web-changing",
];

#[derive(Default)]
struct MockState {
    submissions: usize,
    releases: Vec<Arc<AtomicBool>>,
}
struct Transport {
    state: Arc<Mutex<MockState>>,
    held: bool,
}
struct Receipt {
    released: Arc<AtomicBool>,
    _texture: nana_gpu::GpuTexture,
}
// SAFETY: this fixture installs no foreign GPU reader. Flags model a separate
// release contract after the adapter's actual GPU copy has completed.
unsafe impl OverlayRelease for Receipt {
    fn is_released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }
}
impl OverlayTransport for Transport {
    type Release = Receipt;
    fn submit(&mut self, texture: &nana_gpu::GpuTexture) -> Result<Receipt, OverlayTransportError> {
        let released = Arc::new(AtomicBool::new(!self.held));
        let mut state = self.state.lock().unwrap();
        state.submissions += 1;
        state.releases.push(Arc::clone(&released));
        Ok(Receipt {
            released,
            _texture: texture.clone(),
        })
    }
}

type Adapter = OverlayHostAdapter<Transport>;

fn adapter(gpu: &GpuContext, id: u64, held: bool) -> BenchResult<(Adapter, Arc<Mutex<MockState>>)> {
    let state = Arc::new(Mutex::new(MockState::default()));
    let targets = (0..2)
        .map(|_| {
            gpu.create_texture(&GpuTextureDescriptor {
                label: Some("independent transport consumer pool"),
                width: EXTENT[0],
                height: EXTENT[1],
                format: GpuTextureFormat::RGBA8_UNORM_SRGB,
                usage: GpuTextureUsages::RENDER_TARGET,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let transport = Transport {
        state: Arc::clone(&state),
        held,
    };
    let mut adapter = OverlayHostAdapter::new(gpu, OutputConsumerId(id), targets, transport)?;
    adapter.set_output_trace_enabled(true);
    Ok((adapter, state))
}

fn transfer(
    gpu: &GpuContext,
    driver: &mut PresentationDriver,
    adapter: &mut Adapter,
) -> BenchResult<bool> {
    let mut frame = gpu
        .try_begin_frame("measured external consumer transfer")
        .ok_or("transport frame slot exhausted")?;
    let Some(plan) = adapter.record(driver, &mut frame)? else {
        frame.discard();
        return Ok(false);
    };
    let submission = frame.submit();
    adapter.bind_submission(plan, &submission)?;
    Ok(true)
}

fn finish_transfer(
    adapter: &mut Adapter,
    state: &Arc<Mutex<MockState>>,
    wanted: usize,
) -> BenchResult<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while state.lock().unwrap().submissions < wanted {
        adapter.poll()?;
        if Instant::now() >= deadline {
            return Err("actual consumer GPU transfer did not complete".into());
        }
        std::thread::yield_now();
    }
    Ok(())
}

pub fn run(gpu: &GpuContext, identity: &str) -> Value {
    let mut cases = BTreeMap::new();
    for workload in WORKLOADS {
        let mut report = match run_case(gpu, workload) {
            Ok(report) => report,
            Err(error) => json!({"status":"error", "error":error.to_string()}),
        };
        report["adapter"] = json!(identity);
        report["params"] = json!({"workload":workload,"frames":FRAMES});
        report["evidence"] = json!("actual-gpu-transfer-mock-foreign-retirement");
        cases.insert(workload, report);
    }
    json!({"schema_version":1,"benchmark":"nana-output-transport-benchmark","transport_cases":cases,
        "foreign_runtime":"mock only; no native-handle import, SteamVR compositor or browser was installed",
        "gpu_timestamp_ms":null,"driver_vram_bytes":null})
}

fn run_case(gpu: &GpuContext, workload: &str) -> BenchResult<Value> {
    let slow = workload == "overlay-slow";
    let changed = matches!(
        workload,
        "overlay-changing" | "overlay-multi" | "web-changing"
    );
    let count = if workload == "overlay-multi" { 2 } else { 1 };
    let document = document(false)?;
    let mut driver = PresentationDriver::new(gpu);
    driver.configure(&(1..=count).map(|id| consumer(gpu, id)).collect::<Vec<_>>())?;
    let mut adapters = (1..=count)
        .map(|id| adapter(gpu, id, slow))
        .collect::<BenchResult<Vec<_>>>()?;
    let mut warmup_leases = Vec::new();
    for frame in 0..WARMUP {
        let revision = if changed || slow && frame < 3 {
            frame as u64 + 1
        } else if slow {
            3
        } else {
            0
        };
        let wanted = render(&mut driver, &document, revision, None, false)?;
        let source = completed_at(&mut driver, OutputConsumerId(1), Some(wanted))?;
        if changed && frame < 3 {
            retain_warmup_slot(&mut warmup_leases, source, 3)?;
        }
        for (adapter, state) in &mut adapters {
            let wanted = state.lock().unwrap().submissions + 1;
            if transfer(gpu, &mut driver, adapter)? {
                finish_transfer(adapter, state, wanted)?;
            }
        }
    }
    let before = adapters
        .iter()
        .map(|(adapter, _)| {
            adapter
                .output_trace()
                .expect("benchmark enabled trace")
                .work
        })
        .collect::<Vec<_>>();
    let submitted_before = adapters
        .iter()
        .map(|(_, state)| state.lock().unwrap().submissions)
        .sum::<usize>();
    let mut producer_work = BTreeMap::new();
    let mut producer_times = Vec::new();
    let mut consumer_times = Vec::new();
    let mut measured_policy = MeasuredPolicy::default();
    let mut producer_painter = MeasuredPainter::default();
    let mut producer_heap = HeapCounts::default();
    let mut consumer_heap = HeapCounts::default();
    let mut deferred = 0;
    for frame in 0..FRAMES {
        let policy_before = gpu.policy().stats();
        let heap_before = HeapCounts::snapshot();
        let at = Instant::now();
        let wanted = render(
            &mut driver,
            &document,
            if changed {
                (frame + WARMUP + 1) as u64
            } else if slow {
                3
            } else {
                0
            },
            None,
            true,
        )?;
        let heap_after = HeapCounts::snapshot();
        let elapsed = at.elapsed();
        producer_heap.observe(heap_before, heap_after);
        producer_times.push(elapsed.as_secs_f64() * 1000.0);
        producer_painter.observe(driver.work())?;
        add(&mut producer_work, work(driver.work())?);
        drop(completed_at(
            &mut driver,
            OutputConsumerId(1),
            Some(wanted),
        )?);
        let mut elapsed = Duration::ZERO;
        for (adapter, state) in &mut adapters {
            let wanted = state.lock().unwrap().submissions + 1;
            let at = Instant::now();
            let heap_before = HeapCounts::snapshot();
            let recorded = transfer(gpu, &mut driver, adapter)?;
            elapsed += at.elapsed();
            consumer_heap.observe(heap_before, HeapCounts::snapshot());
            if recorded {
                finish_transfer(adapter, state, wanted)?;
            } else if adapter.last_state() == OverlayAdapterState::Deferred {
                deferred += 1;
            } else if changed {
                return Err("changed output unexpectedly skipped its consumer transfer".into());
            }
        }
        consumer_times.push(elapsed.as_secs_f64() * 1000.0);
        measured_policy.observe(policy_before, gpu.policy().stats());
    }
    let mut consumer_work = BTreeMap::new();
    for ((adapter, _), before) in adapters.iter().zip(before) {
        let after = observation(
            adapter
                .output_trace()
                .expect("benchmark enabled trace")
                .work,
        )?;
        let before = observation(before)?;
        add(
            &mut consumer_work,
            after
                .into_iter()
                .map(|(key, value)| {
                    let previous = before[&key];
                    (key, value - previous)
                })
                .collect(),
        );
    }
    let submitted = adapters
        .iter()
        .map(|(_, state)| state.lock().unwrap().submissions)
        .sum::<usize>()
        - submitted_before;
    let mut recovered = 0;
    if slow {
        for (adapter, state) in &mut adapters {
            for release in &state.lock().unwrap().releases {
                release.store(true, Ordering::Release);
            }
            let wanted = state.lock().unwrap().submissions + 1;
            if transfer(gpu, &mut driver, adapter)? {
                finish_transfer(adapter, state, wanted)?;
                recovered += 1;
            }
        }
    }
    Ok(
        json!({"status":"ok","producer_work":producer_work,"consumer_work":consumer_work,
        "producer_call_ms":percentiles(producer_times),"consumer_call_ms":percentiles(consumer_times),"timing_samples":FRAMES,
        "measurement_gpu_policy":measured_policy,"warmup_frames":WARMUP,"producer_painter":producer_painter.report(),
        "producer_rust_heap":producer_heap.report(FRAMES,"producer calls only"),"consumer_rust_heap":consumer_heap.report(FRAMES,"sum of each frame's installed-consumer record/bind intervals; completion polling and aggregation excluded"),
        "proof":{"attempts":FRAMES,"consumers":count,"scene_primitives":document.scene().primitive_count(),"foreign_submissions":submitted,"deferred_attempts":deferred,"recovered_after_release":recovered,"independent_target_slots":count*2,"foreign_retirement":"mock","actual_platform_import":false},
        "timing_scope":"producer render/submit and consumer record/bind calls separately; real completion polling outside timings; consumer timing sums all installed consumers",
        "foreign_gpu_waits":null,"gpu_timestamp_ms":null,"driver_vram_bytes":null}),
    )
}
