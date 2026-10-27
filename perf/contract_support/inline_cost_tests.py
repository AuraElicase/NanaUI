"""Fixed-machine plain-label ratio gates fail on regressions and missing data."""
import copy
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from .comparison import evaluate_inline_cost_manifest


def self_test_inline_cost() -> list[str]:
    errors = []
    with TemporaryDirectory() as directory:
        root = Path(directory)
        report = {"schema_version": 1, "profile": "release", "cases": [
            {"kind": kind, "nodes": nodes, "entities": nodes + 1,
             "retained_live_bytes": 10000, "idle_frame_p95_ns": 100,
             "idle_allocations": 64000} for kind in ("text", "button") for nodes in (1000, 10000)]}
        manifest = {"machine": "contract-fixture", "before": [f"before-{i}.json" for i in range(3)],
                    "after": [f"after-{i}.json" for i in range(3)]}
        path = root / "manifest.json"
        path.write_text(json.dumps(manifest))
        for name in manifest["before"]:
            (root / name).write_text(json.dumps(report))
        for field, value, expected in ((None, None, 0), ("idle_frame_p95_ns", 106, 1),
                                       ("retained_live_bytes", 10801, 1), ("idle_allocations", 64001, 1)):
            changed = copy.deepcopy(report)
            if field:
                changed["cases"][0][field] = value
            for name in manifest["after"]:
                (root / name).write_text(json.dumps(changed))
            if evaluate_inline_cost_manifest(path)[1] != expected:
                errors.append(f"inline cost gate did not reject {field}")
        changed["cases"].pop()
        (root / manifest["after"][0]).write_text(json.dumps(changed))
        try:
            evaluate_inline_cost_manifest(path)
            errors.append("inline cost gate accepted a missing workload")
        except ValueError:
            pass
        row_report = copy.deepcopy(report)
        row_report["cases"] = row_report["cases"][:2]
        for case in row_report["cases"]:
            case["kind"] = "list_item"
        manifest["workload_family"] = "list_item"
        path.write_text(json.dumps(manifest))
        for version in ("before", "after"):
            for name in manifest[version]:
                (root / name).write_text(json.dumps(row_report))
        if evaluate_inline_cost_manifest(path)[1] != 0:
            errors.append("component cost rejected the complete row workload family")
        for family in ("labeled_value", "card"):
            manifest["workload_family"] = family
            path.write_text(json.dumps(manifest))
            for case in row_report["cases"]:
                case["kind"] = family
            for version in ("before", "after"):
                for name in manifest[version]:
                    (root / name).write_text(json.dumps(row_report))
            if evaluate_inline_cost_manifest(path)[1] != 0:
                errors.append(f"component cost rejected {family}")
        manifest["workload_family"] = "unknown"
        path.write_text(json.dumps(manifest))
        try:
            evaluate_inline_cost_manifest(path)
            errors.append("component cost accepted an unknown workload family")
        except ValueError:
            pass
    return errors
