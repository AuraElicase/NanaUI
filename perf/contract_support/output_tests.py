"""Negative tests for measured-output gates; synthetic reports test the checker only."""
from __future__ import annotations
import copy
from pathlib import Path
from .output_extractors import ENCODES_PER_FRAME, OUTPUT_COUNTERS, HEAP_COUNTERS, MEASURED_POLICY_COUNTERS, PAINTER_COUNTERS, PAINTER_RESOURCE_COUNTERS, WINDOW_CHECKS, extract_output
from .invariants import judge_runner_invariants
from .schema import load_catalog, load_scenario

# Keep the acceptance budget independent from scenario rows. Deleting an eq0
# row must fail this checker, not silently remove that row's mutation test.
_EXPECTED_WORK = {
    "direct": {"ui_renders": 240, "submissions": 240},
    "static": {"idle_reuse_frames": 240},
    "multi": {"ui_renders": 240, "submissions": 240, "content_revisions": 240},
    "nested": {"idle_reuse_frames": 240},
    "conversion": {"ui_renders": 240, "submissions": 480, "content_revisions": 240,
                   "extra_passes": 240, "gpu_convert_passes": 240},
    "slow": {"deferred": 240},
    "recovery": {"ui_renders": 240, "submissions": 240, "content_revisions": 240,
                 "target_recreates": 3},
    "incompatible": {"ui_renders": 480, "submissions": 240, "content_revisions": 480},
}


def _set_path(block, path, value):
    parts = path.split(".")
    for part in parts[:-1]:
        block = block.setdefault(part, {})
    block[parts[-1]] = value


def _delete_path(block, path):
    parts = path.split(".")
    for part in parts[:-1]:
        block = block[part]
    del block[parts[-1]]


def _painter(skipped=False, encodes_per_frame=1):
    return {"status":"skipped" if skipped else "measured", "encoded_frames":0 if skipped else 240 * encodes_per_frame,
            "skipped_frames":240 if skipped else 0, "work":None if skipped else {key:0 for key in PAINTER_COUNTERS}}


def _self_test_output(root: Path) -> list[str]:
    errors = []
    for scenario_id in load_catalog(root).get("nana_output_ids", []):
        scenario = load_scenario(scenario_id, root)
        workload = scenario["params"]["workload"]
        for key in OUTPUT_COUNTERS:
            expected = _EXPECTED_WORK[workload].get(key, 0)
            rows = [row for row in scenario["invariants"] if row["path"] == "output_work." + key]
            if len(rows) != 1 or rows[0].get("op") != "eq" or rows[0].get("value") != expected or rows[0].get("status") != "required":
                errors.append(f"{scenario_id}: acceptance budget for {key} must remain required eq {expected}")
        case = {"status":"ok", "evidence":"actual-window-gpu" if workload == "direct" else "actual-gpu", "adapter":"synthetic checker input; never fixture evidence", "params":scenario["params"].copy(), "output_work":{key:0 for key in OUTPUT_COUNTERS}, "proof":{}, "timing_ms":{"p50":1.0,"p95":2.0,"p99":3.0}, "timing_samples":240}
        case["measurement_rust_heap"] = {"status":"measured", "samples":240, "scope":"synthetic test interval", "counts":{key:0 for key in HEAP_COUNTERS}}
        if workload == "direct":
            case["producer_painter"] = _painter()
            case["proof"]["window_cases"] = {name: True for name in WINDOW_CHECKS}
        if workload != "direct":
            case["measurement_gpu_policy"] = {key:0 for key in MEASURED_POLICY_COUNTERS}
            case["producer_painter"] = {"status":"not instrumented", "work":None} if workload in {"slow", "recovery"} else _painter(workload in {"static", "nested"}, ENCODES_PER_FRAME.get(workload, 1))
        if workload == "incompatible":
            case["painter_resources"] = {"status":"measured", **{key:0 for key in PAINTER_RESOURCE_COUNTERS}}
        if workload == "nested":
            case["parent_measurement_rust_heap"] = copy.deepcopy(case["measurement_rust_heap"])
            case["consumer_call_ms"] = {"p50":1.0,"p95":2.0,"p99":3.0}
            case["consumer_timing_samples"] = 240
            case["parent_painter"] = _painter()
            case["parent_measurement_gpu_policy"] = {key:0 for key in MEASURED_POLICY_COUNTERS}
        for row in scenario["invariants"]:
            _set_path(case, row["path"], row["value"])
        payload = {"output_cases":{workload:case}}
        if workload == "direct":
            payload["passed"] = True
        def extract(report):
            return extract_output(scenario, {"output":report}, {"output":Path("synthetic checker input")})
        accepted = extract(payload)
        if judge_runner_invariants(accepted, root=root).get("decision") != "ok":
            errors.append(f"{scenario_id}: valid checker input rejected")
        # Every observed output field is gated, including formerly unchecked
        # resolve/convert/deferred work. This survives a deleted scenario row.
        for key in OUTPUT_COUNTERS:
            bad = copy.deepcopy(payload)
            bad["output_cases"][workload]["output_work"][key] = _EXPECTED_WORK[workload].get(key, 0) + 1
            try:
                if judge_runner_invariants(extract(bad), root=root).get("decision") != "failed":
                    errors.append(f"{scenario_id}: unexpected {key} work accepted")
            except KeyError:
                pass
        for row in scenario["invariants"]:
            bad = copy.deepcopy(payload)
            _set_path(bad["output_cases"][workload], row["path"], row["value"] + (1 if row["op"] == "eq" else -1))
            try:
                if extract(bad).get("status") != "error":
                    errors.append(f"{scenario_id}: invariant {row['name']} cannot reject")
            except KeyError:
                pass  # Strict metric validation also rejects a violated gate.

        for evidence in ["mock-contract", "unavailable", "actual-gpu" if workload == "direct" else "actual-window-gpu"]:
            bad = copy.deepcopy(payload); bad["output_cases"][workload]["evidence"] = evidence
            if extract(bad).get("status") != "unsupported":
                errors.append(f"{scenario_id}: {evidence} read as measured GPU success")
        required = ["adapter", "proof", "timing_ms", "measurement_rust_heap.scope", "measurement_rust_heap.samples", "producer_painter"] + ["measurement_rust_heap.counts."+key for key in HEAP_COUNTERS] + ["output_work."+key for key in OUTPUT_COUNTERS]
        if workload != "direct":
            required += ["measurement_gpu_policy."+key for key in MEASURED_POLICY_COUNTERS] + ["producer_painter", "timing_samples"]
        if workload in {"direct", "multi", "conversion", "incompatible"}:
            required += ["producer_painter.work."+key for key in PAINTER_COUNTERS]
        if workload == "incompatible":
            required += ["painter_resources.status"] + ["painter_resources."+key for key in PAINTER_RESOURCE_COUNTERS]
        if workload == "nested":
            required += ["consumer_call_ms", "consumer_timing_samples", "parent_measurement_rust_heap"]
            required += ["parent_painter.work."+key for key in PAINTER_COUNTERS]
            required += ["parent_measurement_gpu_policy."+key for key in MEASURED_POLICY_COUNTERS]
        for path in required:
            bad = copy.deepcopy(payload); _delete_path(bad["output_cases"][workload], path)
            try:
                extract(bad)
                errors.append(f"{scenario_id}: missing {path} accepted as measured/zero")
            except KeyError:
                pass
        if workload == "direct":
            # Diagnostic scope is a code-level evidence-classification gate,
            # separate from whether the real GPU workload itself was correct.
            for decoration in ({}, {"scope":None}, {"scope":"complete-six-phase","allocator_observation":"on",
                    "timing_samples_detail":None,"stage_profile":None,"consumer_trace":None,"producer_trace":None}):
                compatible=copy.deepcopy(payload);compatible.update(decoration)
                if judge_runner_invariants(extract(compatible),root=root).get("decision") != "ok":
                    errors.append(f"{scenario_id}: historical/default null diagnostics compatibility rejected")
            for owner in ("report","case"):
                for marker in ("timing_samples_detail","stage_profile","consumer_trace","producer_trace"):
                    for value in ({},[],False,0,{"samples":[1]}):
                        diagnostic=copy.deepcopy(payload)
                        target=diagnostic if owner=="report" else diagnostic["output_cases"][workload]
                        target[marker]=value
                        if extract(diagnostic).get("status") != "error":
                            errors.append(f"{scenario_id}: diagnostic {owner}.{marker} accepted as default timing")
                for marker,values in (("scope",("direct-only; six-phase acceptance not executed","unknown",False)),("allocator_observation",("off",False,0))):
                    for value in values:
                        diagnostic=copy.deepcopy(payload)
                        target=diagnostic if owner=="report" else diagnostic["output_cases"][workload]
                        target[marker]=value
                        if extract(diagnostic).get("status") != "error":
                            errors.append(f"{scenario_id}: diagnostic {owner}.{marker} accepted as default timing")
            diagnostic=copy.deepcopy(payload)
            diagnostic["scope"]="direct-only; six-phase acceptance not executed"
            diagnostic["output_cases"][workload]["params"]["frames"]=1200
            if extract(diagnostic).get("status") != "error":
                errors.append(f"{scenario_id}: long diagnostic scope was downgraded to unavailable instead of error")
            for invalid in [None, False, 1, "true"]:
                bad = copy.deepcopy(payload); bad["passed"] = invalid
                if extract(bad).get("status") != "error":
                    errors.append(f"{scenario_id}: absent/malformed Window passed flag accepted")
            bad = copy.deepcopy(payload); del bad["passed"]
            if extract(bad).get("status") != "error":
                errors.append(f"{scenario_id}: missing Window passed flag accepted")
            for name in WINDOW_CHECKS:
                for invalid in [None, False, 1, "true"]:
                    bad = copy.deepcopy(payload); bad["output_cases"][workload]["proof"]["window_cases"][name] = invalid
                    try:
                        extract(bad)
                        errors.append(f"{scenario_id}: missing/malformed {name} topology proof accepted")
                    except KeyError:
                        pass
            for key in PAINTER_COUNTERS:
                bad = copy.deepcopy(payload); bad["output_cases"][workload]["producer_painter"]["work"][key] = None
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: null direct painter {key} accepted")
                except KeyError:
                    pass
        painter_blocks = [] if workload in {"slow", "recovery"} else ["producer_painter"]
        if workload == "nested":
            painter_blocks.append("parent_painter")
        for painter_key in painter_blocks:
            for count_key in ["encoded_frames", "skipped_frames"]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][painter_key][count_key] += 1
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: inconsistent {painter_key}.{count_key} accepted")
                except KeyError:
                    pass
        observed_blocks = [("proof", row["path"].split(".", 1)[1]) for row in scenario["invariants"] if row["path"].startswith("proof.")]
        if workload != "direct":
            observed_blocks += [("gpu_policy", key) for key in ("frame_slot_waits", "frame_slot_stalls", "upload_ring_waits")]
        for block_key, count_key in observed_blocks:
            for invalid in [None, True, False, 0.0, "0", -1]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][block_key][count_key] = invalid
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: malformed {block_key}.{count_key} accepted")
                except KeyError:
                    pass
        heap_blocks = ["measurement_rust_heap"] + (["parent_measurement_rust_heap"] if workload == "nested" else [])
        for heap_key in heap_blocks:
            for invalid in [None, True, 240.0, "240", -1, 239]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][heap_key]["samples"] = invalid
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: malformed {heap_key}.samples accepted")
                except KeyError:
                    pass
            for invalid in [None, True, 0.0, "0", -1]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][heap_key]["counts"]["alloc_calls"] = invalid
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: malformed {heap_key} allocator count accepted")
                except KeyError:
                    pass
        timing_blocks = [("timing_ms", "timing_samples")]
        if workload == "nested":
            timing_blocks.append(("consumer_call_ms", "consumer_timing_samples"))
        for timing_key, samples_key in timing_blocks:
            for invalid in [None, True, "0", {}, -1, float("nan"), float("inf")]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][timing_key]["p50"] = invalid
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: {timing_key} invalid percentile {invalid!r} accepted")
                except KeyError:
                    pass
            for values in [(3, 2, 1), (1, 3, 2)]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][timing_key] = dict(zip(("p50", "p95", "p99"), values))
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: unordered {timing_key} accepted")
                except KeyError:
                    pass
            for invalid in [None, True, 239, 241, "240"]:
                bad = copy.deepcopy(payload); bad["output_cases"][workload][samples_key] = invalid
                try:
                    extract(bad)
                    errors.append(f"{scenario_id}: invalid {samples_key} accepted")
                except KeyError:
                    pass
        if workload in {"static", "nested"}:
            bad = copy.deepcopy(payload); bad["output_cases"][workload]["producer_painter"]["work"] = {key:0 for key in PAINTER_COUNTERS}
            try:
                extract(bad)
                errors.append(f"{scenario_id}: fabricated zero painter counters accepted when no encode ran")
            except KeyError:
                pass
        bad = copy.deepcopy(payload); bad["output_cases"][workload]["params"]["frames"] = 1
        try:
            extract(bad)
            errors.append(f"{scenario_id}: empty/short workload accepted as 240 frames")
        except KeyError:
            pass
        del accepted["proof"]
        if judge_runner_invariants(accepted, root=root).get("decision") != "failed":
            errors.append(f"{scenario_id}: missing required proof did not fail closed")
    return errors
