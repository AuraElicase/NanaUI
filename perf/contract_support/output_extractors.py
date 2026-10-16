"""Issue #242 measured output reports; mock or unavailable is never GPU proof."""
from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Mapping
from .reports import envelope

OUTPUT_WORKLOADS = frozenset({"direct", "static", "multi", "nested", "conversion", "slow", "recovery"})
OUTPUT_COUNTERS = frozenset({"extra_passes", "gpu_copies", "gpu_copy_bytes", "target_recreates", "content_revisions", "idle_reuse_frames", "resolve_count", "gpu_convert_passes", "plan_rebuilds", "ui_renders", "submissions", "cpu_readbacks", "deferred"})
MEASURED_POLICY_COUNTERS = frozenset({"samples", "upload_bytes", "upload_writes", "upload_copies", "buffer_reallocations", "frame_slot_waits", "frame_slot_stalls", "upload_ring_waits"})
HEAP_COUNTERS = frozenset({"alloc_calls", "alloc_requested_bytes", "dealloc_calls", "dealloc_bytes", "realloc_calls", "realloc_old_bytes", "realloc_requested_bytes"})
PAINTER_COUNTERS = frozenset({"draw_calls", "draw_batches", "batch_rebuilds", "gpu_upload_bytes", "gpu_buffer_reallocations"})
WINDOW_CHECKS = frozenset({"direct_surface", "compatible_shared_render", "incompatible_separate_renders", "detach_restores_direct", "resize_matches_output", "hidden_static_zero_output"})


def _require_timing(block: Any, samples: Any, label: str) -> None:
    if isinstance(samples, bool) or not isinstance(samples, int) or samples != 240:
        raise KeyError(f"{label} must prove exactly 240 measured timing samples")
    if not isinstance(block, dict):
        raise KeyError(f"output report omitted {label}")
    values = [block.get(key) for key in ("p50", "p95", "p99")]
    if any(isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0 for value in values):
        raise KeyError(f"{label} percentiles must be finite nonnegative numbers")
    if not values[0] <= values[1] <= values[2]:
        raise KeyError(f"{label} percentiles must satisfy p50 <= p95 <= p99")


def _require_counts(block: Any, fields: frozenset[str], label: str) -> None:
    if not isinstance(block, dict):
        raise KeyError(f"output GPU report omitted {label}")
    for key in fields:
        value = block.get(key)
        if isinstance(value, bool) or not isinstance(value, int) or value < 0:
            raise KeyError(f"{label}.{key} must be an observed nonnegative integer; never invent zero")


def _require_heap(block: Any, label: str) -> None:
    _require_counts(block, frozenset({"samples"}), label)
    if block.get("status") != "measured" or block["samples"] != 240:
        raise KeyError(f"{label} must prove 240 measured Rust GlobalAlloc intervals")
    if not isinstance(block.get("scope"), str) or not block["scope"]:
        raise KeyError(f"{label} omitted allocator observation scope")
    _require_counts(block.get("counts"), HEAP_COUNTERS, label + ".counts")


def _require_painter(block: Any, label: str, skipped: bool) -> None:
    if not isinstance(block, dict):
        raise KeyError(f"output GPU report omitted {label}")
    _require_counts(block, frozenset({"encoded_frames", "skipped_frames"}), label)
    if (block["encoded_frames"], block["skipped_frames"]) != ((0, 240) if skipped else (240, 0)):
        raise KeyError(f"{label} must account for exactly 240 expected measured encode/skip observations")
    if skipped:
        if block.get("status") != "skipped" or block.get("work") is not None:
            raise KeyError(f"{label} with no producer encode must explicitly report skipped/null counters")
    else:
        if block.get("status") != "measured":
            raise KeyError(f"{label} encoded without measured painter evidence")
        _require_counts(block.get("work"), PAINTER_COUNTERS, label + ".work")


def extract_output(scenario: Mapping[str, Any], reports: Mapping[str, Mapping[str, Any]], source_paths: Mapping[str, Path]) -> dict[str, Any]:
    report = reports.get("output")
    workload = scenario["params"]["workload"]
    if report is None or not isinstance(report.get("output_cases"), dict):
        raise KeyError("nana-output-benchmark output_cases report required")
    case = report["output_cases"].get(workload)
    if not isinstance(case, dict):
        raise KeyError(f"output report omitted workload={workload}")
    if workload == "direct":
        # Timing/trace observers and direct-only experiments are useful
        # diagnostic evidence, but cannot enter the default six-phase timing
        # contract. Historical reports omitted these keys; explicit null keeps
        # that compatibility. Reject even empty/false non-null observer blocks.
        for source in (report, case):
            diagnostic = next((key for key in ("timing_samples_detail", "stage_profile", "consumer_trace", "producer_trace") if source.get(key) is not None), None)
            if diagnostic is not None or source.get("scope") not in (None, "complete-six-phase") or source.get("allocator_observation") not in (None, "on"):
                return envelope(runner="nana", status="error", scenario_id=scenario["id"], scenario=scenario,
                    error="diagnostic Window timing/trace/scope is excluded from the default six-phase performance contract")
    if case.get("params") != scenario["params"]:
        raise KeyError(f"output {workload} workload/frames do not match scenario; no rescaling")
    evidence = case.get("evidence")
    expected_evidence = "actual-window-gpu" if workload == "direct" else "actual-gpu"
    if workload == "direct" and report.get("passed") is not True:
        return envelope(runner="nana", status="error", scenario_id=scenario["id"], scenario=scenario, error="real Window topology probe failed")
    if case.get("status") == "error":
        return envelope(runner="nana", status="error", scenario_id=scenario["id"], scenario=scenario, error=str(case.get("error") or "output workload failed"))
    if case.get("status") != "ok" or evidence != expected_evidence:
        return envelope(runner="nana", status="unsupported", scenario_id=scenario["id"], scenario=scenario, unsupported_reason=case.get("unsupported_reason") or f"{workload} requires {expected_evidence}; observed {evidence}", sections={"evidence": evidence})
    if not isinstance(case.get("adapter"), str) or not case["adapter"]:
        raise KeyError("output GPU report omitted actual adapter/backend identity")
    counters = case.get("output_work")
    _require_counts(counters, OUTPUT_COUNTERS, "output_work")
    proof = case.get("proof")
    if not isinstance(proof, dict):
        raise KeyError("output report omitted actual consumer proof")
    # These are observations, not permissive JSON numbers: bool is an int in
    # Python and must not establish a nonempty workload or a zero wait count.
    proof_counts = frozenset(row["path"].split(".", 1)[1] for row in scenario["invariants"] if row["path"].startswith("proof."))
    _require_counts(proof, proof_counts, "proof")
    if workload == "direct":
        checks = proof.get("window_cases")
        if not isinstance(checks, dict) or any(checks.get(name) is not True for name in WINDOW_CHECKS):
            raise KeyError("real Window report must prove every topology phase; absent/false is not success")
    else:
        _require_counts(case.get("gpu_policy"), frozenset({"frame_slot_waits", "frame_slot_stalls", "upload_ring_waits"}), "gpu_policy")
    timing_samples = case.get("timing_samples", proof.get("presented_frames") if workload == "direct" else None)
    _require_timing(case.get("timing_ms"), timing_samples, "timing_ms")
    if workload == "nested":
        _require_timing(case.get("consumer_call_ms"), case.get("consumer_timing_samples"), "consumer_call_ms")
    sections = {"output_work":{key:counters[key] for key in sorted(OUTPUT_COUNTERS)}, "proof":proof, "evidence":evidence, "adapter":case["adapter"], "gpu_policy":case.get("gpu_policy"), "timing_samples":timing_samples}
    _require_heap(case.get("measurement_rust_heap"), "measurement_rust_heap")
    sections["measurement_rust_heap"] = case["measurement_rust_heap"]
    if workload == "direct":
        _require_painter(case.get("producer_painter"), "producer_painter", skipped=False)
        sections["producer_painter"] = case["producer_painter"]
    if workload != "direct":
        _require_counts(case.get("measurement_gpu_policy"), MEASURED_POLICY_COUNTERS, "measurement_gpu_policy")
        sections["measurement_gpu_policy"] = case["measurement_gpu_policy"]
        painter = case.get("producer_painter")
        if workload in {"slow", "recovery"}:
            if not isinstance(painter, dict) or painter.get("status") != "not instrumented" or painter.get("work") is not None:
                raise KeyError("surface-internal painter metrics must explicitly remain not instrumented/null")
        else:
            _require_painter(painter, "producer_painter", skipped=workload in {"static", "nested"})
        sections["producer_painter"] = painter
        if workload == "nested":
            _require_painter(case.get("parent_painter"), "parent_painter", skipped=False)
            _require_counts(case.get("parent_measurement_gpu_policy"), MEASURED_POLICY_COUNTERS, "parent_measurement_gpu_policy")
            _require_heap(case.get("parent_measurement_rust_heap"), "parent_measurement_rust_heap")
            sections["parent_measurement_rust_heap"] = case["parent_measurement_rust_heap"]
            sections["consumer_timing_samples"] = case["consumer_timing_samples"]
            sections["parent_painter"] = case["parent_painter"]
            sections["parent_measurement_gpu_policy"] = case["parent_measurement_gpu_policy"]
    return envelope(runner="nana", status="ok", scenario_id=scenario["id"], scenario=scenario,
        equivalence="same-scenario", source_binary="nana-output-benchmark" if workload != "direct" else "presentation-output-probe",
        source_report=str(source_paths.get("output", "")), metrics={"producer_call_ms":case.get("timing_ms"), "consumer_call_ms":case.get("consumer_call_ms")},
        mapping_notes=["Counters are measurement-window totals after initialization/warmup. CPU call timings are observations, not GPU execution time or portable CI thresholds.", str(case.get("timing_scope", proof.get("timing_scope", "producer call elapsed wall time")))], sections=sections)
