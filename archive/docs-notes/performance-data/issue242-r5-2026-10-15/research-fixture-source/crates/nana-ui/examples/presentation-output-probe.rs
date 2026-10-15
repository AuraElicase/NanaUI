//! Actual-window presentation/output acceptance. Run with `--output <path>`.
//! Every direct sample is a submitted and presented native Surface frame.

#[path = "../src/bin/support/output_allocator.rs"]
mod output_allocator;
use output_allocator::Counts as HeapCounts;
#[path = "support/execution_bridge.rs"]
mod execution_bridge;
#[path = "support/presentation_timing.rs"]
mod presentation_timing;

use std::convert::Infallible;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

use nana_ui::runtime::view::{entity_ref, widget, with_refs};
use nana_ui::runtime::{DocumentId, Entity, RuntimeDocument, Stack, Text};
use nana_ui::{
    DocumentAccessError, FrameDemand, GpuBackendPolicy, HostFailure, MaterialEffect,
    RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor, WindowHandle,
    WindowOutputConfig, WindowOutputExtent, WindowOutputFrame, run_runtime,
};
use nana_ui_core::{LengthSpec, WorkCounters};
use nana_ui_platform::{WindowEvent, WindowId};

const WARMUP: usize = 30;
const DIRECT_FRAMES: usize = 240;
const OTHER_FRAMES: usize = 60;
const HIDDEN_SETTLE: Duration = Duration::from_millis(200);
const HIDDEN_PERIOD: Duration = Duration::from_millis(1000);
const WATCHDOG: Duration = Duration::from_secs(60);
const STAGE_NAMES: [&str; 11] = [
    "prepare",
    "settle",
    "reserve",
    "acquire",
    "resources",
    "plan",
    "encode",
    "submit",
    "post_submit",
    "present",
    "finish",
];

#[derive(serde::Serialize)]
struct StageSample {
    wall_ms: f64,
    thread_cpu_ms: Option<f64>,
    stages_ns: [u64; 11],
}

fn stage_values() -> [u64; 11] {
    use nana_ui::diagnostics::framework::host::*;
    [
        &PRESENTATION_PREPARE_NS,
        &PRESENTATION_SETTLE_NS,
        &PRESENTATION_RESERVE_NS,
        &PRESENTATION_ACQUIRE_NS,
        &PRESENTATION_RESOURCES_NS,
        &PRESENTATION_PLAN_NS,
        &PRESENTATION_ENCODE_NS,
        &PRESENTATION_SUBMIT_NS,
        &PRESENTATION_POST_SUBMIT_NS,
        &PRESENTATION_PRESENT_NS,
        &PRESENTATION_FINISH_NS,
    ]
    .map(|metric| metric.value())
}

fn thread_cpu_time() -> Option<Duration> {
    presentation_timing::thread_cpu_time()
}
const CASES: [&str; 6] = [
    "direct",
    "compatible",
    "incompatible",
    "detached",
    "resize",
    "hidden-static",
];

#[derive(Debug, Default)]
struct StableProducer(AtomicUsize, AtomicBool);

impl nana_ui::SceneResourceProducer for StableProducer {
    fn preparation_version(&self, _: &nana_ui::runtime::CustomRenderNode) -> Option<u64> {
        Some(1)
    }

    fn encode(
        &self,
        _: &nana_ui::runtime::CustomRenderNode,
        _: nana_ui::SceneResourceEncodeContext<'_>,
    ) -> Result<(), String> {
        if self.1.swap(false, Ordering::Relaxed) {
            return Err("intentional trace recovery probe".into());
        }
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

#[derive(Default, serde::Serialize)]
struct PainterCounts {
    draw_calls: usize,
    draw_batches: usize,
    batch_rebuilds: usize,
    gpu_upload_bytes: usize,
    gpu_buffer_reallocations: usize,
}

#[derive(Default, serde::Serialize)]
struct Observed {
    frames: usize,
    #[serde(skip)]
    heap: HeapCounts,
    #[serde(skip)]
    painter: PainterCounts,
    #[serde(skip)]
    painter_encoded_frames: usize,
    #[serde(skip)]
    painter_skipped_frames: usize,
    #[serde(skip)]
    missing_painter_observations: usize,
    extra_passes: usize,
    gpu_copies: usize,
    gpu_copy_bytes: usize,
    cpu_readbacks: usize,
    ui_renders: usize,
    submissions: usize,
    target_recreates: usize,
    plan_rebuilds: usize,
    content_revisions: usize,
    idle_reuse_frames: usize,
    resolve_count: usize,
    gpu_convert_passes: usize,
    deferred: usize,
    missing_observations: usize,
    output_frames: usize,
    #[serde(skip)]
    times: Vec<f64>,
}

impl Observed {
    fn record(&mut self, work: WorkCounters, elapsed: Duration) {
        if let Some(renders) = work.output_ui_renders.filter(|renders| *renders > 0) {
            let metrics = [
                work.draw_calls,
                work.draw_batches,
                work.batch_rebuilds,
                work.gpu_upload_bytes,
                work.gpu_buffer_reallocations,
            ];
            self.missing_painter_observations += usize::from(metrics.iter().any(Option::is_none));
            self.painter_encoded_frames += renders;
            // A missing metric makes the painter report error/null below;
            // the partial sums cannot establish successful zero evidence.
            self.painter.draw_calls += work.draw_calls.unwrap_or(0);
            self.painter.draw_batches += work.draw_batches.unwrap_or(0);
            self.painter.batch_rebuilds += work.batch_rebuilds.unwrap_or(0);
            self.painter.gpu_upload_bytes += work.gpu_upload_bytes.unwrap_or(0);
            self.painter.gpu_buffer_reallocations += work.gpu_buffer_reallocations.unwrap_or(0);
        } else {
            self.painter_skipped_frames += 1;
        }
        let observed = [
            work.output_extra_passes,
            work.output_gpu_copies,
            work.output_gpu_copy_bytes,
            work.output_cpu_readbacks,
            work.output_ui_renders,
            work.output_submissions,
            work.output_target_recreates,
            work.output_plan_rebuilds,
            work.output_content_revisions,
            work.output_idle_reuse_frames,
            work.output_resolve_count,
            work.output_gpu_convert_passes,
            work.output_deferred,
        ];
        self.missing_observations += usize::from(observed.iter().any(Option::is_none));
        // Missing remains a failed observation; these sums never establish
        // evidence unless missing_observations is zero.
        self.frames += 1;
        self.extra_passes += work.output_extra_passes.unwrap_or(0);
        self.gpu_copies += work.output_gpu_copies.unwrap_or(0);
        self.gpu_copy_bytes += work.output_gpu_copy_bytes.unwrap_or(0);
        self.cpu_readbacks += work.output_cpu_readbacks.unwrap_or(0);
        self.ui_renders += work.output_ui_renders.unwrap_or(0);
        self.submissions += work.output_submissions.unwrap_or(0);
        self.target_recreates += work.output_target_recreates.unwrap_or(0);
        self.plan_rebuilds += work.output_plan_rebuilds.unwrap_or(0);
        self.content_revisions += work.output_content_revisions.unwrap_or(0);
        self.idle_reuse_frames += work.output_idle_reuse_frames.unwrap_or(0);
        self.resolve_count += work.output_resolve_count.unwrap_or(0);
        self.gpu_convert_passes += work.output_gpu_convert_passes.unwrap_or(0);
        self.deferred += work.output_deferred.unwrap_or(0);
        self.times.push(elapsed.as_secs_f64() * 1000.0);
    }

    fn painter_report(&self) -> serde_json::Value {
        let measured = self.missing_painter_observations == 0 && self.painter_encoded_frames > 0;
        serde_json::json!({"status":if measured {"measured"} else {"error"},
            "encoded_frames":self.painter_encoded_frames,"skipped_frames":self.painter_skipped_frames,
            "missing_observations":self.missing_painter_observations,
            "work":measured.then_some(&self.painter),
            "scope":"measured real Window Scene painter encodes from Runtime WorkCounters; setup and warmup excluded; GPU buffer reallocations are not process malloc"})
    }

    fn timing(&self) -> serde_json::Value {
        let mut values = self.times.clone();
        values.sort_by(f64::total_cmp);
        if values.is_empty() {
            return serde_json::Value::Null;
        }
        let percentile = |percent: usize| values[((values.len() - 1) * percent) / 100];
        serde_json::json!({"p50": percentile(50), "p95": percentile(95), "p99": percentile(99)})
    }

    fn zero_extra_work(&self) -> bool {
        self.missing_observations == 0
            && self.extra_passes == 0
            && self.gpu_copies == 0
            && self.cpu_readbacks == 0
            && self.target_recreates == 0
            && self.plan_rebuilds == 0
    }
}

fn record_timing(
    samples: &mut presentation_timing::Phase,
    ordinal: usize,
    warmup: bool,
    included: bool,
    wall: f64,
    cpu: Option<f64>,
    execution: Option<presentation_timing::ExecutionInterval>,
) {
    if samples.execution_samples.is_some() {
        samples.record_observed(ordinal, warmup, included, wall, cpu, execution);
    } else {
        samples.record(ordinal, warmup, included, wall, cpu);
    }
}

struct Probe {
    document: RuntimeDocument,
    label: Entity<Text>,
    window: Option<WindowHandle>,
    started: Instant,
    phase_started: Instant,
    frame_started: Instant,
    frame_heap_started: HeapCounts,
    phase: usize,
    phase_frames: usize,
    observations: Vec<Observed>,
    tick: u64,
    original_extent: [u32; 2],
    resized_extent: Option<[u32; 2]>,
    last_output_extent: Option<[u32; 2]>,
    backend: String,
    unused_producers: bool,
    producers: Option<nana_ui::SceneResourceProducerRegistry>,
    stable_producer: Option<Arc<StableProducer>>,
    hidden_producer_calls: Option<usize>,
    host_textures: Option<nana_ui::HostTextureRegistry>,
    stage_samples: Option<Vec<Vec<StageSample>>>,
    frame_rate: NonZeroU32,
    watchdog: Duration,
    thread_cpu_started: Option<Duration>,
    timing_options: presentation_timing::Options,
    timing_samples: Option<presentation_timing::Series>,
    execution_observer: Option<presentation_timing::ExecutionObserver>,
    execution_started: Option<presentation_timing::ExecutionSnapshot>,
    bridge: Option<execution_bridge::Bridge>,
    focus_schedule: execution_bridge::Schedule,
    category_treatment: bool,
    neutral_pid: i32,
    last_activation: i32,
    last_anchor_frame: Option<usize>,
    context_started: Option<execution_bridge::Context>,
    context_request: Option<i32>,
    context_samples: Option<Vec<execution_bridge::ContextSample>>,
    clock_anchors: Option<Vec<execution_bridge::Anchor>>,
    trace_samples: Option<Vec<(usize, nana_ui::output::OutputTraceSnapshot)>>,
    trace_abort_expected: bool,
    trace_abort_seen: bool,
}

impl Probe {
    fn finish(&self, failure: Option<&str>) -> ! {
        // Cold trailing marker bounds the final execution interval. Capture
        // failures remain explicit; an attempted marker is not trace proof.
        let restored_category = self.bridge.as_ref().and_then(|bridge| bridge.restore());
        let final_anchor = self.bridge.as_ref().map(|bridge| {
            bridge.anchor(
                self.last_anchor_frame
                    .map_or(1, |frame| (frame / 120 + 2) as u64),
            )
        });
        let direct = &self.observations[0];
        let compatible = &self.observations[1];
        let incompatible = &self.observations[2];
        let detached = &self.observations[3];
        let resized = &self.observations[4];
        let hidden = &self.observations[5];
        let checks = serde_json::json!({
            "direct_surface": direct.frames == self.timing_options.measured && direct.zero_extra_work()
                && direct.ui_renders == self.timing_options.measured && direct.submissions == self.timing_options.measured
                && direct.missing_painter_observations == 0
                && direct.painter_encoded_frames == self.timing_options.measured
                && direct.painter.draw_calls > 0 && direct.painter.draw_batches > 0,
            "compatible_shared_render": compatible.frames == OTHER_FRAMES
                && compatible.missing_observations == 0 && compatible.ui_renders == OTHER_FRAMES
                && compatible.submissions == OTHER_FRAMES && compatible.extra_passes == OTHER_FRAMES
                && compatible.gpu_copies == 0 && compatible.cpu_readbacks == 0,
            "incompatible_separate_renders": incompatible.frames == OTHER_FRAMES
                && incompatible.missing_observations == 0 && incompatible.ui_renders == OTHER_FRAMES * 2
                && incompatible.submissions == OTHER_FRAMES && incompatible.cpu_readbacks == 0,
            "detach_restores_direct": detached.frames == OTHER_FRAMES && detached.zero_extra_work()
                && detached.ui_renders == OTHER_FRAMES && detached.submissions == OTHER_FRAMES,
            "resize_matches_output": resized.frames == OTHER_FRAMES
                && self.resized_extent.is_some_and(|extent| extent != self.original_extent)
                && self.last_output_extent == self.resized_extent
                && resized.ui_renders == OTHER_FRAMES && resized.cpu_readbacks == 0,
            "hidden_static_zero_output": hidden.frames >= 20 && hidden.zero_extra_work()
                && hidden.ui_renders == 0 && hidden.submissions == 0 && hidden.content_revisions == 0
                && hidden.output_frames == 0 && hidden.idle_reuse_frames >= 20
                && self.stable_producer.as_ref().is_none_or(|producer|
                    self.hidden_producer_calls.is_some_and(|calls| calls > 0
                        && producer.0.load(Ordering::Relaxed) == calls)),
            "trace_failure_recovery": !self.trace_abort_expected || (self.trace_abort_seen && self.trace_samples.as_ref().is_some_and(|samples| {
                use nana_ui::output::{OutputTraceState, OutputPath};
                samples.iter().any(|(phase, trace)| *phase == 0 && trace.path == OutputPath::DirectSurface
                    && trace.state == OutputTraceState::Failed && trace.content_revision == 0 && trace.waits.is_none())
                && samples.iter().any(|(phase, trace)| *phase == 0 && trace.path == OutputPath::DirectSurface
                    && trace.state == OutputTraceState::Submitted && trace.content_revision > 0)
            })),
            "consumer_trace_attribution": self.trace_samples.as_ref().is_none_or(|samples| {
                use nana_ui::output::{OutputPath, OutputTraceRole};
                samples.iter().all(|(_, trace)| trace.role != OutputTraceRole::Consumer || trace.work.ui_renders == 0)
                    && samples.iter().any(|(phase, trace)| *phase == 0 && trace.path == OutputPath::DirectSurface && trace.work.ui_renders > 0)
                    && samples.iter().any(|(phase, trace)| *phase == 1 && trace.role == OutputTraceRole::Consumer && trace.work.extra_passes > 0)
            }),
        });
        let timing_sampling_complete = self.timing_samples.as_ref().is_none_or(|series| {
            series.phases.iter().enumerate().all(|(phase, samples)| {
                let expected = if phase == 0 {
                    self.timing_options.measured
                } else if phase == 5 || self.timing_options.direct_only {
                    0
                } else {
                    OTHER_FRAMES
                };
                samples.dropped_samples == 0
                    && samples.execution_complete()
                    && samples
                        .samples
                        .iter()
                        .filter(|sample| sample.included_in_measurement)
                        .count()
                        == expected
            })
        });
        let checks = if self.timing_options.direct_only {
            serde_json::json!({"direct_surface": checks["direct_surface"]})
        } else {
            checks
        };
        let passed = failure.is_none()
            && timing_sampling_complete
            && checks
                .as_object()
                .unwrap()
                .values()
                .all(|value| value == true);
        let direct_ok = checks["direct_surface"] == true;
        let report = serde_json::json!({
            "schema_version": 1,
            "benchmark": "presentation-output-probe",
            "output_cases": {"direct": {
                "status": if direct_ok { "ok" } else { "error" },
                "error": if direct_ok { None } else { Some("actual-window direct work invariants failed") },
                "adapter": self.backend,
                "evidence": "actual-window-gpu",
                "params": {"workload": "direct", "frames": self.timing_options.measured},
                "output_work": direct,
                "timing_ms": direct.timing(),
                "timing_samples": direct.times.len(),
                "warmup_frames": self.timing_options.warmup,
                "measurement_rust_heap": direct.heap.report(direct.frames, "prepare_window_frame through window_frame_presented; aggregation and serialization excluded"),
                "producer_painter": direct.painter_report(),
                "measurement_gpu_policy": null,
                "gpu_timestamp_ms": null, "driver_vram_bytes": null, "process_heap_allocations": null,
                "proof": {"consumer_samples": direct.frames, "presented_frames": direct.frames,
                    "unused_producer_registry": self.unused_producers,
                    "stable_producer_encode_calls": self.stable_producer.as_ref().map(|producer| producer.0.load(Ordering::Relaxed)),
                    "hidden_producer_encode_calls_at_settle": self.hidden_producer_calls,
                    "scene_primitives": self.document.scene().primitives().count(),
                    "backend": self.backend, "window_cases": checks, "phase_work": self.observations,
                    "resize_extent": self.resized_extent, "original_extent": self.original_extent,
                    "timing_scope": "prepare_window_frame through window_frame_presented"}
            }},
            "passed": passed, "failure": failure,
            "timing_sampling_complete":timing_sampling_complete,
            "scope": if self.timing_options.direct_only { "direct-only; six-phase acceptance not executed" } else { "complete-six-phase" },
            "requested_frame_rate": self.frame_rate.get(),
            "allocator_observation":if self.timing_options.allocator_enabled { "on" } else { "off" },
            "timing_samples_detail": self.timing_samples.as_ref().map(|samples| serde_json::json!({
                "schema_version":1,"phase_names":CASES,"series":samples,
                "execution_observer":self.execution_observer.as_ref().map(|observer| &observer.metadata),
                "clock_at_report":self.execution_observer.as_ref().map(|observer| observer.clock_anchor()),
                "context_samples":self.context_samples,"clock_anchors":self.clock_anchors,
                "clock_final_anchor":final_anchor,
                "focus_schedule":self.focus_schedule,"neutral_pid":self.neutral_pid,
                "policy_treatment":if self.category_treatment {"task-category-only"} else {"activation"},
                "restore_category_result":restored_category,
                "native_context_scope":"archive helper opt-in; native queries may allocate or communicate; spans explicitly recorded; no native context calls by default; activation treatment occurs before wall timer, has its own transition exclusion in analysis",
                "warmup_frames":self.timing_options.warmup,"measured_frames":self.timing_options.measured,
                "diagnostics_enabled":self.stage_samples.is_some(),
                "allocator_observation":if self.timing_options.allocator_enabled { "on" } else { "off" },
                "scope":"opt-in successful presented-frame samples including warmup; interval is between actual prepare callbacks, ordinal gaps expose attempts without presentation; hidden prepare-only period has no presented samples",
                "observer":"bounded preallocated storage; opt-in CPU/kind4 reads and interval validation add overhead; native context snapshots and sparse anchors occur outside the original wall interval; execution interval has independently bracketed endpoints and excludes those native queries; no diagnostics metrics enabled by these flags",
                "qos_scope":"cold requested QoS plus per-endpoint requested QoS only with --execution-samples; effective QoS unavailable via public getter, not CPU frequency/core residency; fixture does not set QoS",
                "gap_scope":"wall minus current-thread CPU; includes off-thread work/scheduling/native waits and clock-observer boundary differences, not a pure drawable wait timer",
                "actual_surface_present_mode":null,"cpu_frequency_hz":null,
            })),
            "stage_profile": self.stage_samples.as_ref().map(|samples| serde_json::json!({
                "names": STAGE_NAMES, "phases": CASES, "samples": samples, "requested_frame_rate": self.frame_rate.get(),
                "scope": "latest host-thread phase gauges, diagnostics enabled; phase wall time includes waits; thread CPU time excludes waits and is available only on macOS",
                "instrumentation": "opt-in; never mixed with default uninstrumented five-round comparisons",
            })),
            "consumer_trace": self.trace_samples.as_ref().map(|samples| samples.iter().map(|(phase, trace)| serde_json::json!({
                "phase": CASES[*phase], "producer": trace.producer, "consumer": trace.consumer.0,
                "role": format!("{:?}", trace.role), "path": format!("{:?}", trace.path),
                "format": format!("{:?}", trace.format), "extent": trace.extent,
                "device_generation": trace.device_generation.get(), "resource_generation": trace.resource_generation,
                "content_revision": trace.content_revision, "state": format!("{:?}", trace.state),
                "deferral_reason": format!("{:?}", trace.deferral_reason), "recreate_reason": format!("{:?}", trace.recreate_reason),
                "work": {"ui_renders": trace.work.ui_renders, "submissions": trace.work.submissions,
                    "extra_passes": trace.work.extra_passes, "gpu_copies": trace.work.gpu_copies,
                    "gpu_copy_bytes": trace.work.gpu_copy_bytes, "cpu_readbacks": trace.work.cpu_readbacks,
                    "target_recreates": trace.work.target_recreates, "deferred": trace.work.deferred,
                    "idle_reuse_frames": trace.work.idle_reuse_frames},
                "waits": trace.waits.map(|waits| serde_json::json!({"frame_slot_waits": waits.frame_slot_waits, "frame_slot_stalls": waits.frame_slot_stalls})),
            })).collect::<Vec<_>>()),
        });
        println!("{report}");
        if let Some(path) = output_path() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        std::process::exit(if passed { 0 } else { 1 });
    }

    fn advance(&mut self, context: &RuntimeProgramContext<()>) -> RuntimeProgramUpdate {
        if self.timing_options.direct_only && self.phase == 0 {
            self.finish(None);
        }
        self.phase += 1;
        self.phase_frames = 0;
        self.phase_started = Instant::now();
        println!("{}", serde_json::json!({"phase": CASES[self.phase]}));
        if self.phase == 4 {
            self.original_extent = [
                context.geometry().physical_size.0,
                context.geometry().physical_size.1,
            ];
            let _request = self.window.as_ref().unwrap().set_size((520.0, 360.0));
        }
        if self.phase == 5 {
            let _request = self.window.as_ref().unwrap().set_visible(false);
        }
        RuntimeProgramUpdate::redraw(WindowId::PRIMARY)
    }
}

impl RuntimeProgram for Probe {
    type Message = ();
    type Error = Infallible;

    fn gpu_backend_policy() -> GpuBackendPolicy {
        GpuBackendPolicy::CompositionCapable
    }
    fn startup_window_material_mode() -> MaterialEffect {
        MaterialEffect::Transparent
    }
    fn window_material_mode(&self) -> MaterialEffect {
        MaterialEffect::Transparent
    }

    fn initialize(context: &RuntimeProgramContext<()>) -> Result<(Self, Vec<()>), Infallible> {
        let id = DocumentId::new(1).unwrap();
        let mut document = RuntimeDocument::new(id);
        let (_, label) = document
            .context_mut()
            .mount_view_root(id, || {
                let label = entity_ref::<Text>();
                let root = widget(Stack::fill_column(8.0).with_layout(|layout| {
                    layout.background = Some([0.12, 0.3, 0.7, 0.6]);
                    layout.padding_left = Some(LengthSpec::Px(20.0));
                    layout.padding_top = Some(LengthSpec::Px(20.0));
                }))
                .children(widget(Text::new("presentation 00000000")).entity_ref(label));
                with_refs(root, label)
            })
            .unwrap();
        let timing_options = presentation_timing::Options::parse(std::env::args().skip(1))
            .expect("valid timing fixture options");
        let frame_rate = {
            let mut arguments = std::env::args().skip(1);
            let mut fps = 120;
            while let Some(arg) = arguments.next() {
                if arg == "--frame-rate" {
                    fps = arguments
                        .next()
                        .expect("frame rate value")
                        .parse()
                        .expect("integer frame rate");
                    assert!(
                        (30..=240).contains(&fps),
                        "probe frame rate must be 30..=240"
                    );
                }
            }
            NonZeroU32::new(fps).unwrap()
        };
        let watchdog = if timing_options.direct_only {
            let nominal = Duration::from_secs_f64(
                (timing_options.warmup + timing_options.measured) as f64
                    / f64::from(frame_rate.get()),
            );
            WATCHDOG.max(nominal.saturating_mul(3) + Duration::from_secs(10))
        } else {
            WATCHDOG
        };
        let execution_observer = timing_options
            .execution_enabled
            .then(presentation_timing::ExecutionObserver::new);
        let focus_schedule = execution_bridge::Schedule::parse(std::env::args().skip(1))
            .expect("valid focus schedule");
        assert!(
            focus_schedule == execution_bridge::Schedule::Passive
                || timing_options.execution_enabled,
            "activation experiments require --execution-samples"
        );
        assert!(
            focus_schedule == execution_bridge::Schedule::Passive
                || timing_options.measured.is_multiple_of(4),
            "activation experiments require equal four blocks"
        );
        let bridge = timing_options
            .execution_enabled
            .then(execution_bridge::Bridge::load)
            .flatten();
        let category_treatment = std::env::args().any(|arg| arg == "--task-category-policy");
        assert!(
            !category_treatment
                || (timing_options.execution_enabled
                    && bridge
                        .as_ref()
                        .is_some_and(|bridge| bridge.category_supported())),
            "category experiments require execution samples and native diagnostic helper"
        );
        let neutral_pid = std::env::var("NANA_R5_NEUTRAL_PID")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        assert!(
            focus_schedule == execution_bridge::Schedule::Passive
                || (bridge.is_some() && (category_treatment || neutral_pid > 0)),
            "activation experiments require archive helper and fixed neutral app PID"
        );
        let context_samples = bridge
            .as_ref()
            .map(|_| Vec::with_capacity(timing_options.warmup + timing_options.measured));
        assert!(
            bridge.is_none()
                || (timing_options.warmup + timing_options.measured).div_ceil(120) < 32,
            "archive clock helper supports at most 31 periodic anchors plus one final anchor"
        );
        let clock_anchors = bridge.as_ref().map(|_| Vec::with_capacity(32));
        let timing_samples = timing_options
            .enabled
            .then(|| presentation_timing::Series::new(timing_options));
        let now = Instant::now();
        let unused_producers = std::env::args().any(|arg| arg == "--probe-unused-producers");
        let trace_abort_expected = std::env::args().any(|arg| arg == "--probe-trace-aborts");
        let stable_producer = (trace_abort_expected
            || std::env::args().any(|arg| arg == "--probe-stable-producer"))
        .then(|| Arc::new(StableProducer::default()));
        if trace_abort_expected {
            stable_producer
                .as_ref()
                .unwrap()
                .1
                .store(true, Ordering::Relaxed);
        }
        let producers = if let Some(producer) = &stable_producer {
            document
                .context_mut()
                .create_component(id, nana_ui::runtime::GpuTextureView::new("probe-stable"))
                .unwrap();
            let mut registry = nana_ui::SceneResourceProducerRegistry::new();
            registry.insert("probe-stable", producer.clone());
            Some(registry)
        } else {
            unused_producers.then(nana_ui::SceneResourceProducerRegistry::new)
        };
        let host_textures = stable_producer.as_ref().map(|_| {
            let texture = context
                .gpu()
                .create_texture(&nana_gpu::GpuTextureDescriptor {
                    label: Some("stable producer window regression fixture"),
                    width: 1,
                    height: 1,
                    format: nana_gpu::GpuTextureFormat::RGBA8_UNORM_SRGB,
                    usage: nana_gpu::GpuTextureUsages::SAMPLED,
                })
                .unwrap();
            let registry = nana_ui::HostTextureRegistry::new();
            registry.register(
                "probe-stable",
                nana_ui::HostTexture::new(1, 1, &texture),
                1,
                1,
                nana_ui::HostTextureAlphaMode::Premultiplied,
            );
            registry
        });
        Ok((
            Self {
                document,
                label,
                window: None,
                started: now,
                phase_started: now,
                frame_started: now,
                frame_heap_started: HeapCounts::default(),
                phase: 0,
                phase_frames: 0,
                observations: (0..CASES.len()).map(|_| Observed::default()).collect(),
                tick: 0,
                original_extent: [0; 2],
                resized_extent: None,
                last_output_extent: None,
                backend: String::new(),
                unused_producers,
                producers,
                stable_producer,
                hidden_producer_calls: None,
                host_textures,
                stage_samples: std::env::args()
                    .any(|arg| arg == "--profile-stages")
                    .then(|| {
                        (0..CASES.len())
                            .map(|_| Vec::with_capacity(DIRECT_FRAMES))
                            .collect()
                    }),
                thread_cpu_started: None,
                timing_options,
                timing_samples,
                execution_observer,
                execution_started: None,
                bridge,
                focus_schedule,
                category_treatment,
                neutral_pid,
                last_activation: 0,
                last_anchor_frame: None,
                context_started: None,
                context_request: None,
                context_samples,
                clock_anchors,
                trace_abort_expected,
                trace_abort_seen: false,
                trace_samples: std::env::args()
                    .any(|arg| arg == "--trace-outputs" || arg == "--probe-trace-aborts")
                    .then(|| Vec::with_capacity(DIRECT_FRAMES * 12)),
                frame_rate,
                watchdog,
            },
            Vec::new(),
        ))
    }

    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok((id == WindowId::PRIMARY).then(|| f(&self.document)))
    }
    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok((id == WindowId::PRIMARY).then(|| f(&mut self.document)))
    }
    fn update(&mut self, _: (), _: &RuntimeProgramContext<()>) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<()>,
    ) -> RuntimeProgramUpdate {
        match event {
            WindowEvent::Ready { .. } => {
                self.window = Some(context.window());
                self.started = Instant::now();
                self.backend = format!(
                    "{:?}: {}",
                    context.gpu().capabilities().backend(),
                    context.gpu().capabilities().adapter_name()
                );
                RuntimeProgramUpdate::redraw(WindowId::PRIMARY)
            }
            WindowEvent::CloseRequested { .. } => {
                self.finish(Some("window closed before acceptance completed"))
            }
            _ => RuntimeProgramUpdate::default(),
        }
    }

    fn frame_demand(&self, _: WindowId) -> FrameDemand {
        FrameDemand::Continuous(self.frame_rate)
    }

    fn output_trace_enabled(&self, _: WindowId) -> bool {
        self.trace_samples.is_some()
    }

    fn window_output_trace(
        &mut self,
        _: WindowId,
        trace: nana_ui::output::OutputTraceSnapshot,
        _: &RuntimeProgramContext<()>,
    ) {
        if let Some(samples) = &mut self.trace_samples {
            samples.push((self.phase, trace));
        }
    }

    fn prepare_window_frame(&mut self, _: WindowId, _: &RuntimeProgramContext<()>) {
        if let Some(bridge) = &self.bridge {
            let requested = self.focus_schedule.requested(
                self.phase_frames + 1,
                self.timing_options.warmup,
                self.timing_options.measured,
            );
            // Leave normal startup policy untouched during category warmup;
            // apply the first category intervention at the measured boundary.
            let requested =
                if self.category_treatment && self.phase_frames < self.timing_options.warmup {
                    0
                } else {
                    requested
                };
            self.context_request = if requested != 0 && requested != self.last_activation {
                self.last_activation = requested;
                Some(bridge.activate(
                    if self.category_treatment {
                        requested + 2
                    } else {
                        requested
                    },
                    self.neutral_pid,
                ))
            } else {
                None
            };
            if self.phase_frames.is_multiple_of(120)
                && self.last_anchor_frame != Some(self.phase_frames)
            {
                self.last_anchor_frame = Some(self.phase_frames);
                if let Some(anchors) = &mut self.clock_anchors
                    && anchors.len() < anchors.capacity()
                {
                    anchors.push(bridge.anchor((self.phase_frames / 120 + 1) as u64));
                }
            }
            self.context_started = bridge.context();
        }
        self.frame_heap_started = HeapCounts::snapshot();
        self.frame_started = Instant::now();
        // Keep the large diagnostic snapshot assignment off the default path.
        // Mapping None still copied its unused payload in the Release binary.
        if let Some(observer) = &self.execution_observer {
            self.execution_started = Some(observer.snapshot());
        }
        if self.stage_samples.is_some() || self.timing_samples.is_some() {
            self.thread_cpu_started = thread_cpu_time();
        }
        if let Some(samples) = &mut self.timing_samples {
            samples.phases[self.phase].prepare(self.frame_started);
        }
        if self.phase == 5 {
            if self.phase_started.elapsed() >= HIDDEN_SETTLE {
                if self.hidden_producer_calls.is_none() {
                    self.hidden_producer_calls = self
                        .stable_producer
                        .as_ref()
                        .map(|producer| producer.0.load(Ordering::Relaxed));
                }
                self.observations[5]
                    .record(self.document.context().last_work_counters(), Duration::ZERO);
            }
            return;
        }
        self.tick += 1;
        let value = format!("presentation {:08}", self.tick);
        self.document
            .context_mut()
            .update_component(self.label, |label, _| label.value = value)
            .unwrap();
    }

    fn window_frame_presented(
        &mut self,
        _: WindowId,
        context: &RuntimeProgramContext<()>,
    ) -> RuntimeProgramUpdate {
        if self.phase == 5 {
            return RuntimeProgramUpdate::default();
        }
        let execution = self.execution_observer.as_ref().map(|observer| {
            let end = observer.snapshot();
            observer.interval(self.execution_started.take(), end)
        });
        let heap_after = HeapCounts::snapshot();
        let thread_cpu_ms = if self.stage_samples.is_some() || self.timing_samples.is_some() {
            self.thread_cpu_started
                .zip(thread_cpu_time())
                .and_then(|(start, end)| end.checked_sub(start))
                .map(|elapsed| elapsed.as_secs_f64() * 1000.0)
        } else {
            None
        };
        self.phase_frames += 1;
        let count = if self.phase == 0 {
            self.timing_options.measured
        } else {
            OTHER_FRAMES
        };
        let warmup = if self.phase == 0 {
            self.timing_options.warmup
        } else {
            WARMUP
        };
        if self.phase_frames <= warmup
            && let Some(samples) = &mut self.timing_samples
        {
            record_timing(
                &mut samples.phases[self.phase],
                self.phase_frames,
                true,
                false,
                self.frame_started.elapsed().as_secs_f64() * 1000.0,
                thread_cpu_ms,
                execution,
            );
        }
        if self.phase_frames > warmup {
            if self.phase == 4 {
                let size = context.geometry().physical_size;
                let extent = [size.0, size.1];
                if extent == self.original_extent {
                    if let Some(samples) = &mut self.timing_samples {
                        record_timing(
                            &mut samples.phases[self.phase],
                            self.phase_frames,
                            false,
                            false,
                            self.frame_started.elapsed().as_secs_f64() * 1000.0,
                            thread_cpu_ms,
                            execution,
                        );
                    }
                    return RuntimeProgramUpdate::redraw(WindowId::PRIMARY);
                }
                self.resized_extent = Some(extent);
            }
            self.observations[self.phase]
                .heap
                .observe(self.frame_heap_started, heap_after);
            self.observations[self.phase].record(
                self.document.context().last_work_counters(),
                self.frame_started.elapsed(),
            );
            if let Some(samples) = &mut self.timing_samples {
                record_timing(
                    &mut samples.phases[self.phase],
                    self.phase_frames,
                    false,
                    true,
                    *self.observations[self.phase].times.last().unwrap(),
                    thread_cpu_ms,
                    execution,
                );
            }
            if let Some(samples) = &mut self.stage_samples {
                samples[self.phase].push(StageSample {
                    wall_ms: *self.observations[self.phase].times.last().unwrap(),
                    thread_cpu_ms,
                    stages_ns: stage_values(),
                });
            }
        }
        let context_after = self.bridge.as_ref().and_then(|bridge| bridge.context());
        if let Some(samples) = &mut self.context_samples
            && samples.len() < samples.capacity()
        {
            samples.push(execution_bridge::ContextSample {
                presented_ordinal: self.phase_frames,
                requested_state: self.last_activation,
                request_result: self.context_request.take(),
                before: self.context_started.take(),
                after: context_after,
            });
        }
        if self.observations[self.phase].frames == count {
            self.advance(context)
        } else {
            RuntimeProgramUpdate::default()
        }
    }

    fn next_wakeup(&self) -> Option<Instant> {
        Some(if self.phase == 5 {
            self.phase_started + HIDDEN_PERIOD
        } else {
            self.started + self.watchdog
        })
    }
    fn wake(&mut self, _: Instant, _: &RuntimeProgramContext<()>) -> RuntimeProgramUpdate {
        if self.phase == 5 && self.phase_started.elapsed() >= HIDDEN_PERIOD {
            self.finish(None);
        }
        if self.started.elapsed() >= self.watchdog {
            self.finish(Some("native window frame watchdog expired"));
        }
        RuntimeProgramUpdate::default()
    }

    fn scene_resource_producers(
        &self,
        _: WindowId,
    ) -> Option<nana_ui::SceneResourceProducerRegistry> {
        self.producers.clone()
    }

    fn host_textures(&self, _: WindowId) -> Option<nana_ui::HostTextureRegistry> {
        self.host_textures.clone()
    }

    fn window_output(&self, _: WindowId) -> Option<WindowOutputConfig> {
        match self.phase {
            0 | 3 => None,
            2 => Some(
                WindowOutputConfig::default().with_extent(WindowOutputExtent::Fixed {
                    width: 320,
                    height: 200,
                }),
            ),
            _ => Some(WindowOutputConfig::default().with_extent(WindowOutputExtent::MatchWindow)),
        }
    }
    fn window_output_frame(
        &mut self,
        _: WindowId,
        frame: &WindowOutputFrame,
        _: &RuntimeProgramContext<()>,
    ) {
        self.last_output_extent = Some(frame.extent());
        if self.phase == 5 && self.phase_started.elapsed() >= HIDDEN_SETTLE {
            self.observations[5].output_frames += 1;
        }
    }
    fn host_failure(&mut self, failure: HostFailure) {
        if self.trace_abort_expected
            && !self.trace_abort_seen
            && matches!(&failure, HostFailure::ResourceProduction { error, .. } if error.contains("intentional trace recovery probe"))
        {
            self.trace_abort_seen = true;
            return;
        }
        self.finish(Some("host failed to render the acceptance window"));
    }
}

fn output_path() -> Option<PathBuf> {
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--output" {
            return arguments.next().map(PathBuf::from);
        }
    }
    None
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    let timing_options = presentation_timing::Options::parse(std::env::args().skip(1))
        .expect("valid timing fixture options");
    output_allocator::configure(timing_options.allocator_enabled)
        .expect("allocator observation is configured once at startup");
    let _diagnostics = std::env::args()
        .any(|arg| arg == "--profile-stages")
        .then(|| {
            use nana_ui::diagnostics::*;
            install(
                DiagnosticsConfig {
                    metrics: true,
                    persist: PersistMode::Off,
                    panic_hook: false,
                    ..Default::default()
                },
                SessionMetadata::new(
                    "dev.nana.presentation-stages",
                    "Presentation stage probe",
                    "0",
                ),
                DiagnosticsPaths::in_memory(),
            )
            .expect("stage probe owns diagnostics instance")
        });
    let mut window = WindowDescriptor::new("NanaUI presentation output acceptance")
        .initial_size(480.0, 320.0)
        .minimum_size(200.0, 160.0)
        .surface(nana_ui::WindowSurfacePreference::Composition);
    window.transparent = true;
    window.shadow = nana_ui::WindowShadow::None;
    window.initial_position = Some((100.0, 100.0));
    run_runtime::<Probe>(window)
}
