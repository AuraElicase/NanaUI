"""Recompute all published numerical evidence; original native identity stays local."""
import copy
import gzip
import hashlib
import importlib.util
import json
import statistics
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SPEC = importlib.util.spec_from_file_location("presentation_cpu", HERE.parents[3] / "perf/presentation_cpu.py")
CPU = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CPU)


def read(name):
    return json.loads((HERE / name).read_text())


def main():
    summary = read("summary.json")
    topology = read("cpu-map.json")
    expected = {case["name"]: case for case in summary["cases"]}
    results = {}
    frozen = HERE / "frozen-frame-classifier.py"
    assert hashlib.sha256(frozen.read_bytes()).hexdigest() == summary["protocol"]["analysis_source_sha256"]
    with tempfile.TemporaryDirectory(prefix="nanaui-issue354-review-") as directory:
        for item in read("reports-manifest.json"):
            compressed = (HERE / item["file"]).read_bytes()
            assert hashlib.sha256(compressed).hexdigest() == item["sha256"]
            data = gzip.decompress(compressed)
            report = json.loads(data)
            name = item["file"].removesuffix(".json.gz")
            (Path(directory) / (name + ".json")).write_bytes(data)
            result = CPU.analyze(report, topology)
            case = expected[name]
            assert item["original_report_sha256"] == case["report_sha256"]
            assert result["frames"] == 2400
            assert result["groups"] == case["tiers"]
            assert result["p_time_fraction_bounds"] == case["p_time_fraction_bounds"]
            assert all(abs(result["cpu_ms"][q] - case["cpu_ms"][q]) < 1e-9 for q in result["cpu_ms"])
            results[name] = result
            # A report with incomplete endpoints cannot pass the reader.
            bad = copy.deepcopy(report)
            next(row for row in bad["timing_samples_detail"]["series"]["phases"][0]["samples"] if row["included_in_measurement"])["cpu_endpoints"] = []
            try:
                CPU.analyze(bad, topology)
            except ValueError:
                pass
            else:
                raise AssertionError("incomplete CPU endpoints accepted")
        ruby = json.loads(subprocess.check_output(["ruby", str(HERE / "review_bounds.rb"), directory], text=True))
        for row in ruby:
            case = expected[row["name"]]
            assert row["p_time_fraction_bounds"] == case["p_time_fraction_bounds"]
            assert row["class_sequence_sha256"] == case["class_sequence_sha256"]
            assert row["frames"] == 2400
            assert all(row["groups"].get(key, 0) == value["frames"] for key, value in case["tiers"].items())
    assert len(results) == len(ruby) == 20
    deltas = []
    for group in summary["groups"]:
        cases = [case for case in summary["cases"] if case["group"] == group["group"]]
        a = [case for case in cases if case["arm"] == "A"]
        b = [case for case in cases if case["arm"] == "B"]
        delta = statistics.mean(case["cpu_ms"]["0.5"] for case in b) - statistics.mean(case["cpu_ms"]["0.5"] for case in a)
        assert abs(delta - group["cpu_ms"]["0.5"]["B_minus_A"]) < 1e-9
        deltas.append(delta)
        assert all(CPU.compare(results[left["name"]], results[right["name"]])["reason"] == "disjoint-execution-time-bounds" for left in a for right in b)
    assert abs(statistics.median(deltas) - .329604) < 1e-9
    assert CPU.compare(results["g3-2-B"], results["g3-3-B"])["unstratified_latency_attribution"] == "needs-source-and-context-control"
    print("20 processes / 48,000 measured frames: hashes, Python/Ruby classifications, exact aggregate bounds and all five group statistics match; original native identities were redacted.")


if __name__ == "__main__":
    main()
