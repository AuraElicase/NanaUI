"""Owner-attributed transport gates; mock retirement never proves a platform."""
from __future__ import annotations

import math
from pathlib import Path
from typing import Any, Mapping
from .output_extractors import OUTPUT_COUNTERS, MEASURED_POLICY_COUNTERS, _require_counts, _require_heap, _require_painter, _require_timing
from .reports import envelope

PORTABLE_TRANSPORTS = frozenset({"overlay-static", "overlay-changing", "overlay-multi", "overlay-slow", "web-static", "web-changing"})
PLATFORM_TRANSPORTS = frozenset({"native-host", "openvr-native", "web-native"})


def _timing(block: Any, samples: int | None = None) -> None:
    if not isinstance(block, dict):
        raise KeyError("actual transport omitted CPU timing observation")
    count = block.get("samples")
    if isinstance(count, bool) or not isinstance(count, int) or count <= 0 or samples is not None and count != samples:
        raise KeyError("actual transport CPU samples do not match the observed workflow")
    values = [block.get(key) for key in ("p50", "p95", "p99")]
    if any(isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0 for value in values) or not values[0] <= values[1] <= values[2]:
        raise KeyError("actual transport CPU quantiles must be finite, nonnegative and ordered")


def extract_transport(scenario: Mapping[str, Any], reports: Mapping[str, Mapping[str, Any]], source_paths: Mapping[str, Path]) -> dict[str, Any]:
    report = reports.get("output_transport")
    workload = scenario["params"]["workload"]
    if not isinstance(report, dict):
        raise KeyError("transport report required")
    if workload in PORTABLE_TRANSPORTS:
        case = (report.get("transport_cases") or {}).get(workload)
        if not isinstance(case, dict):
            raise KeyError(f"transport report omitted {workload}")
    else:
        case = report
    if case.get("status") == "unsupported":
        return envelope(runner="nana", status="unsupported", scenario_id=scenario["id"], scenario=scenario, unsupported_reason=case.get("unsupported_reason") or case.get("reason") or "actual external platform unavailable")
    accepted_status = "active" if workload == "openvr-native" else "ok"
    if case.get("status") != accepted_status:
        return envelope(runner="nana", status="error", scenario_id=scenario["id"], scenario=scenario, error=case.get("error") or "transport did not complete its requested workflow")
    expected = "actual-gpu-transfer-mock-foreign-retirement" if workload in PORTABLE_TRANSPORTS else {"native-host":"actual-native-host-gpu", "openvr-native":"actual-openvr-gpu", "web-native":"actual-electron-webgpu"}[workload]
    if case.get("evidence") != expected:
        raise KeyError(f"{workload} requires {expected}; mock/unavailable cannot prove external platform conformance")
    if not isinstance(case.get("adapter"), str) or not case["adapter"]:
        raise KeyError("transport omitted actual adapter identity")
    sections = {"evidence":expected,"adapter":case["adapter"],"platform_conformance":workload in PLATFORM_TRANSPORTS}
    metrics = {}
    if workload in PLATFORM_TRANSPORTS:
        if case.get("platform") != "windows":
            raise KeyError("maintained native transport requires actual Windows platform identity")
        sections["platform"] = case["platform"]
        _require_counts(case, frozenset({"fixture_version","warmup_frames","device_generation"}), "platform_fixture")
        if case["fixture_version"] != 1 or case["warmup_frames"] != 30 or case["device_generation"] < 1:
            raise KeyError("actual platform fixture/version/device generation/warmup differs")
        extent = case.get("extent")
        expected_extent = {"native-host":[1280,720],"openvr-native":[128,96],"web-native":[640,360]}[workload]
        if not isinstance(extent,list) or any(isinstance(value,bool) or not isinstance(value,int) or value <= 0 for value in extent) or extent != expected_extent:
            raise KeyError("actual transport extent differs from the maintained BGRA workload")
        sections.update({key:case[key] for key in ("fixture_version","warmup_frames","device_generation","extent")})
    if workload in PORTABLE_TRANSPORTS:
        if case.get("params") != scenario["params"] or case.get("warmup_frames") != 30:
            raise KeyError("portable transport workload/warmup differs; no rescaling")
        for owner in ("producer_work", "consumer_work"):
            _require_counts(case.get(owner), OUTPUT_COUNTERS, owner)
            sections[owner] = case[owner]
        _require_counts(case.get("proof"), frozenset({"attempts","consumers","scene_primitives","foreign_submissions","deferred_attempts","recovered_after_release","independent_target_slots"}), "proof")
        if case["proof"].get("foreign_retirement") != "mock" or case["proof"].get("actual_platform_import") is not False or case.get("foreign_gpu_waits") is not None:
            raise KeyError("portable workload must declare mock foreign retirement and unknown foreign GPU waits")
        for key in ("producer_call_ms", "consumer_call_ms"):
            _require_timing(case.get(key), case.get("timing_samples"), key)
            metrics[key] = case[key]
        _require_counts(case.get("measurement_gpu_policy"), MEASURED_POLICY_COUNTERS, "measurement_gpu_policy")
        _require_painter(case.get("producer_painter"), "producer_painter", skipped=workload not in {"overlay-changing","overlay-multi","web-changing"})
        for key in ("producer_rust_heap", "consumer_rust_heap"):
            _require_heap(case.get(key), key)
        sections.update({key:case[key] for key in ("measurement_gpu_policy","producer_painter","producer_rust_heap","consumer_rust_heap")})
    elif workload == "native-host":
        _require_counts(case, frozenset({"frames","source_calls"}), "native_host")
        if case.get("fixture") != "native-host-output" or case.get("warmup_frames") != 30:
            raise KeyError("native host needs its real DX12/D3D11 workload with 30 warmup tokens")
        if case.get("native_api") != "DX12-NT-D3D11":
            raise KeyError("native-host fixture did not identify its actual native API")
        _require_counts(case.get("producer_work"), frozenset({"ui_renders","draw_calls","submissions","cpu_readbacks","target_recreates","gpu_copies","gpu_copy_bytes"}), "producer_work")
        _require_counts(case.get("consumer_work"), frozenset({"gpu_copies","gpu_copy_bytes","gpu_ready_waits","gpu_release_signals","target_recreates","validation_readbacks"}), "consumer_work")
        _require_counts(case.get("proof"), frozenset({"native_handle_imports","explicit_cpu_fence_waits","production_cpu_readbacks"}), "proof")
        if case["proof"].get("consumer_completed") is not True:
            raise KeyError("native host did not observe actual final consumer release completion")
        for owner in ("producer_work", "consumer_work"):
            if case[owner]["gpu_copy_bytes"] != case[owner]["gpu_copies"] * 1280 * 720 * 4:
                raise KeyError("native host copy bytes do not match actual fixed BGRA extent")
            sections[owner] = case[owner]
        if case["source_calls"] < 240:
            raise KeyError("native source CPU workload is truncated")
        producer = case["producer_work"]
        if producer["submissions"] != case["source_calls"] or not case["source_calls"] <= producer["ui_renders"] <= 2*case["source_calls"]:
            raise KeyError("native host must submit one physical Window frame per source call and render at most its Window plus retained target")
        _timing(case.get("producer_window_cpu_ms"),case["source_calls"])
        _timing(case.get("consumer_record_cpu_ms"), 240)
        metrics = {key:case[key] for key in ("producer_window_cpu_ms","consumer_record_cpu_ms")}
        sections["frames"] = case.get("frames")
        sections["source_calls"] = case["source_calls"]
    elif workload == "openvr-native":
        if case.get("fixture") != "openvr-native-output" or case.get("mode") != "shared_atomic":
            raise KeyError("OpenVR continuous workload requires the actual shared_atomic reference")
        if case.get("interface") != "IVROverlay_028" or case.get("compositor") != "steamvr" or case.get("backend") != "dx12-d3d11":
            raise KeyError("OpenVR fixture pin/native path differs")
        if case.get("alpha_encoding") != "gamma-premultiplied" or case.get("overlay_premultiplied") is not True or case.get("workload_alpha") != "opaque":
            raise KeyError("OpenVR reference must identify gamma-premultiplied encoding, the actual premultiplied overlay flag and maintained opaque workload")
        sections.update({key:case[key] for key in ("alpha_encoding","overlay_premultiplied","workload_alpha")})
        fields = frozenset({"attempts","warmup_attempts","total_attempts","total_consumer_updates","requested_frames","frames_received","submitted_to_compositor","consumer_updates_completed","deferred","consumer_slots","consumer_slot_reuses","consumer_resource_retirements","consumer_target_allocations","producer_ui_renders","producer_ui_submissions","producer_draw_calls","producer_native_submissions","producer_native_copies","producer_native_copy_bytes","consumer_gpu_copies","consumer_gpu_copy_bytes","cpu_readbacks","explicit_cpu_fence_waits"})
        _require_counts(case, fields, "openvr")
        _require_counts(case.get("static"), frozenset({"attempts","producer_ui_renders","producer_submissions","producer_native_copies","consumer_gpu_copies"}), "static")
        if case.get("continuous_reuse_supported") is not True or case.get("compositor_retirement_proven") is not False:
            raise KeyError("OpenVR atomic updates must not claim allocation retirement")
        if case["producer_ui_submissions"] != case["producer_ui_renders"] or case["producer_ui_submissions"] < case["producer_native_submissions"] or case["producer_ui_renders"] > case["attempts"]:
            raise KeyError("OpenVR producer counters do not match actual source attempts")
        if case["total_attempts"] != case["attempts"] + case["warmup_attempts"] or case["warmup_attempts"] < 30:
            raise KeyError("OpenVR measured attempts include/omit warmup")
        setup=case.get("setup_scope")
        _require_counts(setup,frozenset({"submitted_to_compositor","consumer_target_allocations"}),"setup_scope")
        _timing(setup.get("native_call_cpu_ms"),1)
        native_call=case.get("native_call_cpu_ms")
        if not isinstance(native_call,dict) or native_call.get("status") != "skipped" or type(native_call.get("samples")) is not int or native_call["samples"] != 0 or any(native_call.get(key) is not None for key in ("p50","p95","p99")):
            raise KeyError("unchanged OpenVR handle has no measured SDK call; do not invent zero timings")
        for key, samples in (("producer_record_cpu_ms",case["attempts"]),("producer_native_record_cpu_ms",240),("consumer_record_cpu_ms",240)):
            _timing(case.get(key), samples); metrics[key] = case[key]
        sections.update({key:case[key] for key in fields})
        sections["static"] = case["static"]
        sections["setup_scope"] = setup
    elif workload == "web-native":
        if case.get("fixture") != "electron-webgpu-output" or case.get("electron_version") != "44.7.2":
            raise KeyError("actual Electron bridge/fixture pin differs")
        work_fields = frozenset({"ui_renders","gpu_copies","gpu_copy_bytes","extra_passes","submissions","cpu_readbacks","target_recreates","deferred"})
        proof_fields = frozenset({"native_handle_imports","ready_observations","foreign_releases","displayed_frames"})
        consumer_fields = frozenset({"displayed_frames","submissions","draw_calls"})
        _require_counts(case.get("output_work"), work_fields, "output_work")
        _require_counts(case.get("setup_work"), work_fields, "setup_work")
        _require_counts(case.get("warmup_output_work"), work_fields, "warmup_output_work")
        _require_counts(case, frozenset({"total_frames"}), "web")
        if case["total_frames"] != 270:
            raise KeyError("Electron total displayed workload must be disjoint 30 warmup plus 240 measured frames")
        for proof_name, count in (("warmup_proof",30),("total_proof",270)):
            proof = case.get(proof_name)
            _require_counts(proof,proof_fields,proof_name)
            if any(proof[key] != count for key in proof_fields) or proof.get("has_pending_gpu_reads") is not False:
                raise KeyError("Electron warmup/total receipts do not prove completed foreign GPU work")
        for name, count in (("warmup_consumer_work",30),("total_consumer_work",270)):
            _require_counts(case.get(name),consumer_fields,name)
            if any(case[name][key] != count for key in consumer_fields):
                raise KeyError("Electron warmup/total consumer submissions differ from actual frames")
        if case["setup_work"]["target_recreates"] != 6 or case["warmup_output_work"]["gpu_copies"] != 30 or case["warmup_output_work"]["gpu_copy_bytes"] != 30*640*360*4:
            raise KeyError("Electron warmup/setup did not initialize the real persistent pool workload")
        sections.update({key:case[key] for key in ("total_frames","setup_work","warmup_output_work","warmup_proof","total_proof","warmup_consumer_work","total_consumer_work")})
        _require_counts(case.get("proof"), frozenset({"native_handle_imports","ready_observations","foreign_releases","displayed_frames"}), "proof")
        _require_counts(case.get("consumer_work"), frozenset({"displayed_frames","submissions","draw_calls"}), "consumer_work")
        if case["proof"].get("has_pending_gpu_reads") is not False:
            raise KeyError("Electron did not retire all foreign GPU reads")
        _require_counts(case, frozenset({"timing_samples","frames"}), "web")
        if not isinstance(case.get("timing_ms"), dict):
            raise KeyError("Web producer omitted CPU call timings")
        _timing({**case["timing_ms"],"samples":case["timing_samples"]})
        if case["timing_samples"] < 240:
            raise KeyError("Web producer timing workload is empty or truncated")
        work = case["output_work"]
        if work["ui_renders"] > case["timing_samples"] or work["submissions"] != work["ui_renders"]+work["gpu_copies"]:
            raise KeyError("Electron source allows at most one UI render per timed call and one separate native submission per copy")
        metrics["producer_call_ms"] = case["timing_ms"]
        _timing(case.get("consumer_record_cpu_ms"),240)
        metrics["consumer_record_cpu_ms"] = case["consumer_record_cpu_ms"]
        sections.update({key:case[key] for key in ("output_work","consumer_work","frames")})
    if "proof" in case:
        sections["proof"] = case["proof"]
    return envelope(runner="nana", status="ok", scenario_id=scenario["id"], scenario=scenario,
        equivalence="same-scenario", source_binary="nana-output-benchmark --transport-only" if workload in PORTABLE_TRANSPORTS else str(case.get("fixture") or "web-output-bridge"),
        source_report=str(source_paths.get("output_transport", "")), metrics=metrics, sections=sections,
        mapping_notes=["Producer and consumer work remain separate. Portable mock retirement proves only a real local GPU-transfer/lifecycle contract, never a native import or external platform.",str(case.get("scope") or case.get("timing_scope") or "CPU call time; not GPU execution duration")])
