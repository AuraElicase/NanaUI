#!/usr/bin/env python3
"""Read public process counters enclosing a real Window CPU16 interval.

Other threads only loosen the bound: main-thread P time cannot exceed the
whole process's P time, nor can its non-P time exceed the process's non-P time.
No thread-group layout, policy recommendation or endpoint CPU sample is used
as an interval-residency measurement. This tool does not declare source cause.
"""
import argparse
import json
import math
from fractions import Fraction
from pathlib import Path

FIELDS = (
    "user_cpu_ticks", "system_cpu_ticks", "performance_user_cpu_ticks",
    "performance_system_cpu_ticks", "instructions", "cycles",
    "performance_instructions", "performance_cycles", "process_start_abstime",
)


def uint(value, name):
    if type(value) is not int or not 0 <= value < 2**64:
        raise ValueError(f"{name}: unsigned integer required")
    return value


def interval(sample, unit):
    """Sufficient lower bounds, conditional on the public counter contract."""
    cpu = uint(sample["thread_cpu_ns"], "thread_cpu_ns")
    if not cpu or unit <= 0:
        raise ValueError("positive CPU duration and Mach timebase required")
    snapshots = sample["process_counter_endpoints"]
    if len(snapshots) != 2:
        raise ValueError("two enclosing process snapshots required")
    values = []
    for snapshot in snapshots:
        if type(snapshot["error"]) is not int or snapshot["error"] != 0:
            raise ValueError("process counter query failed")
        values.append({key: uint(snapshot["values"][key], key) for key in FIELDS})
    a, b = values
    if not a["process_start_abstime"] or a["process_start_abstime"] != b["process_start_abstime"]:
        raise ValueError("process incarnation changed")
    delta = {key: b[key] - a[key] for key in FIELDS if key != "process_start_abstime"}
    if any(value < 0 for value in delta.values()):
        raise ValueError("counter regressed")
    total = (delta["user_cpu_ticks"] + delta["system_cpu_ticks"]) * unit
    performance = (delta["performance_user_cpu_ticks"] + delta["performance_system_cpu_ticks"]) * unit
    # Conservative integer-quantization allowance. This does not bound an
    # undocumented kernel accounting lag; inconsistent intervals are rejected.
    guard = 8 * unit
    if total + guard < cpu or performance > total:
        raise ValueError("process counters do not enclose the thread interval")
    if not delta["instructions"] or not delta["cycles"]:
        raise ValueError("instruction/cycle counters unavailable")
    if delta["performance_instructions"] > delta["instructions"] or delta["performance_cycles"] > delta["cycles"]:
        raise ValueError("performance counters exceed process totals")
    lower_cpu, upper_cpu = max(Fraction(0), cpu - guard), cpu + guard
    p_lower = max(Fraction(0), (lower_cpu - (total - performance) - guard) / upper_cpu)
    e_lower = max(Fraction(0), (lower_cpu - performance - guard) / upper_cpu)
    if p_lower >= Fraction(95, 100):
        group = "P95-bound"
    elif e_lower >= Fraction(95, 100):
        group = "E95-bound"
    else:
        group = "ambiguous"
    return {
        "group": group, "p_lower": p_lower, "e_lower": e_lower,
        "cpu_ns": cpu, "process_cpu_ns": total,
        "process_instructions": delta["instructions"], "process_cycles": delta["cycles"],
        "incarnation": a["process_start_abstime"],
    }


def quantile(values, q):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * q) - 1)]


def condition_signature(report):
    direct = report["output_cases"]["direct"]
    return {
        "surface": report["surface_configuration"],
        "frame_rate": report["requested_frame_rate"],
        "presented_counter": report["presented_counter"],
        "cpu_observer": report["cpu_endpoint_observation"],
        "process_observer": report["process_counter_observation"],
        "stage_observer": report["stage_profile"] is not None,
        "work": direct["output_work"], "painter": direct["producer_painter"],
        "warmup": direct["warmup_frames"], "params": direct["params"],
        "adapter": direct["adapter"],
    }


def analyze(report, topology):
    if {entry["cluster_type"] for entry in topology.values()} != {"P", "E"}:
        raise ValueError("captured heterogeneous P/E topology required")
    if not all(report[key] is True for key in ("passed", "execution_observations_complete", "cpu_endpoint_observation", "process_counter_observation")):
        raise ValueError("complete requested observations required")
    if report["surface_observation_error"] is not None or report["direct_configuration_changes"]:
        raise ValueError("surface changed or was not observed")
    numer, denom = (uint(value, "timebase") for value in report["mach_timebase"])
    if not numer or not denom:
        raise ValueError("positive Mach timebase required")
    unit = Fraction(numer, denom)
    phase = report["timing_samples_detail"]["series"]["phases"][0]
    if phase["dropped_samples"]:
        raise ValueError("dropped samples")
    samples = [sample for sample in phase["samples"] if sample["included_in_measurement"]]
    expected = report["timing_samples_detail"]["measured_frames"]
    if not samples or len(samples) != expected:
        raise ValueError("incomplete measured frames")
    rows, incarnations = [], set()
    for sample in samples:
        for point in sample["cpu_endpoints"]:
            if type(point["error"]) is not int or point["error"] != 0 or str(uint(point["cpu_number"], "cpu_number")) not in topology:
                raise ValueError("CPU endpoint missing from captured topology")
        row = interval(sample, unit)
        row["endpoint_types"] = "".join(topology[str(point["cpu_number"])]["cluster_type"] for point in sample["cpu_endpoints"])
        rows.append(row)
        incarnations.add(row["incarnation"])
    if len(incarnations) != 1:
        raise ValueError("more than one process incarnation")
    groups = {}
    for group in ("P95-bound", "E95-bound", "ambiguous"):
        selected = [row for row in rows if row["group"] == group]
        groups[group] = {"frames": len(selected), "fraction": len(selected) / len(rows)}
        if selected:
            groups[group].update({
                "cpu_ms": {str(q): float(quantile([row["cpu_ns"] for row in selected], q)) / 1e6 for q in (.5, .95, .99)},
                "process_instructions_p50": quantile([row["process_instructions"] for row in selected], .5),
                "process_cycles_p50": quantile([row["process_cycles"] for row in selected], .5),
                "process_cpu_ms_p50": float(quantile([row["process_cpu_ns"] for row in selected], .5)) / 1e6,
                "matching_endpoint_type_frames": sum(row["endpoint_types"] == ("PP" if group == "P95-bound" else "EE") for row in selected) if group != "ambiguous" else None,
            })
    return {
        "frames": len(rows), "incarnation_stable": True, "groups": groups,
        "cpu_ms": {str(q): quantile([row["cpu_ns"] for row in rows], q) / 1e6 for q in (.5, .95, .99)},
        "conditions": condition_signature(report),
        "scope": "Sufficient main-thread execution-tier bounds from enclosing public process CPU counters; all other threads only loosen bounds. Not TG tenure, requested policy, main-thread instruction counts or a source-cause verdict.",
    }


def compare(left, right, mix_tolerance=0):
    if left["conditions"] != right["conditions"]:
        reason = "different-observed-conditions"
    elif max(abs(left["groups"][key]["fraction"] - right["groups"][key]["fraction"]) for key in left["groups"]) > mix_tolerance:
        reason = "different-execution-tier-mixture"
    elif max(left["groups"]["ambiguous"]["fraction"], right["groups"]["ambiguous"]["fraction"]) > .1:
        reason = "insufficient-execution-tier-coverage"
    else:
        reason = None
    return {
        "unstratified_latency_attribution": "blocked" if reason else "needs-source-and-context-control",
        "reason": reason,
        "cpu_p50_delta_ms": right["cpu_ms"]["0.5"] - left["cpu_ms"]["0.5"],
        "scope": "A matched mixture alone never proves a source regression or repair.",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--cpu-map", required=True, type=Path)
    parser.add_argument("--compare", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    topology = json.loads(args.cpu_map.read_text())
    result = analyze(json.loads(args.report.read_text()), topology)
    if args.compare:
        other = analyze(json.loads(args.compare.read_text()), topology)
        result = {"left": result, "right": other, "comparison": compare(result, other)}
    text = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
