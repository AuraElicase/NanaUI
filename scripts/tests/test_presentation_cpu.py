import copy
import importlib.util
import unittest
from fractions import Fraction
from pathlib import Path

PATH = Path(__file__).resolve().parents[2] / "perf/presentation_cpu.py"
SPEC = importlib.util.spec_from_file_location("presentation_cpu", PATH)
CPU = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CPU)


def frame(total=1_200_000, performance=0, cpu=1_000_000):
    start = {key: 100 for key in CPU.FIELDS}
    end = dict(start)
    end["user_cpu_ticks"] += total
    end["performance_user_cpu_ticks"] += performance
    end["instructions"] += 2_000_000
    end["cycles"] += 3_000_000
    return {
        "thread_cpu_ns": cpu,
        "process_counter_endpoints": [
            {"error": 0, "values": start}, {"error": 0, "values": end},
        ],
    }


class ExecutionBounds(unittest.TestCase):
    def test_enclosing_all_process_time_on_one_tier_bounds_the_main_thread(self):
        self.assertEqual(CPU.interval(frame(), Fraction(1))["group"], "E95-bound")
        self.assertEqual(CPU.interval(frame(performance=1_200_000), Fraction(1))["group"], "P95-bound")

    def test_other_threads_can_only_weaken_the_main_thread_bound(self):
        result = CPU.interval(frame(total=2_000_000, performance=1_000_000), Fraction(1))
        self.assertEqual(result["group"], "ambiguous")
        self.assertEqual(result["p_lower"], 0)
        self.assertEqual(result["e_lower"], 0)

    def test_sufficient_bounds_do_not_require_the_process_itself_to_be_95_percent(self):
        self.assertEqual(CPU.interval(frame(performance=40_000), Fraction(1))["group"], "E95-bound")
        self.assertEqual(CPU.interval(frame(performance=1_160_000), Fraction(1))["group"], "P95-bound")
        self.assertEqual(CPU.interval(frame(performance=60_000), Fraction(1))["group"], "ambiguous")

    def test_mach_tick_conversion_is_exact_and_not_assumed_nanoseconds(self):
        converted = CPU.interval(frame(total=28_800, performance=28_800), Fraction(125, 3))
        self.assertEqual(converted["process_cpu_ns"], 1_200_000)
        self.assertEqual(converted["group"], "P95-bound")

    def test_query_failure_incarnation_change_and_unavailable_counters_are_rejected(self):
        cases = []
        value = frame(); value["process_counter_endpoints"][1]["error"] = 5; cases.append(value)
        value = frame(); value["process_counter_endpoints"][1]["values"]["process_start_abstime"] += 1; cases.append(value)
        value = frame(); value["process_counter_endpoints"][1]["values"]["instructions"] = 100; cases.append(value)
        for value in cases:
            with self.assertRaises(ValueError):
                CPU.interval(value, Fraction(1))

    def test_incoherent_and_regressing_intervals_are_rejected_without_dropping_rows(self):
        for value in [frame(total=800_000), frame(performance=1_300_000)]:
            with self.assertRaises(ValueError):
                CPU.interval(value, Fraction(1))
        value = frame()
        value["process_counter_endpoints"][1]["values"]["cycles"] = 0
        with self.assertRaises(ValueError):
            CPU.interval(value, Fraction(1))
        for value in [True, -1, 2**64, 1.0, None]:
            with self.assertRaises(ValueError):
                CPU.uint(value, "counter")


class AttributionGuard(unittest.TestCase):
    def setUp(self):
        self.report = {"conditions": {"extent": [480, 320]},
            "groups": {"P95-bound": {"fraction": .98}, "E95-bound": {"fraction": .01}, "ambiguous": {"fraction": .01}},
            "p_time_fraction_bounds": {"lower": {"numerator": 98, "denominator": 100}, "upper": {"numerator": 1, "denominator": 1}},
            "cpu_ms": {"0.5": .4}}

    def test_unequal_tier_mixtures_block_total_p50_attribution(self):
        other = copy.deepcopy(self.report)
        other["groups"]["P95-bound"]["fraction"] = .01
        other["groups"]["E95-bound"]["fraction"] = .98
        other["cpu_ms"]["0.5"] = .8
        other["p_time_fraction_bounds"] = {"lower": {"numerator": 0, "denominator": 1}, "upper": {"numerator": 1, "denominator": 50}}
        result = CPU.compare(self.report, other)
        self.assertEqual(result["reason"], "disjoint-execution-time-bounds")
        self.assertEqual(result["unstratified_latency_attribution"], "blocked")

    def test_matching_mixtures_never_prove_source_cause(self):
        result = CPU.compare(self.report, self.report)
        self.assertIsNone(result["reason"])
        self.assertEqual(result["unstratified_latency_attribution"], "needs-source-and-context-control")

    def test_ambiguous_labels_are_not_called_actual_residency_differences(self):
        other = copy.deepcopy(self.report)
        other["groups"] = {"P95-bound": {"fraction": 0}, "E95-bound": {"fraction": 0}, "ambiguous": {"fraction": 1}}
        other["p_time_fraction_bounds"]["lower"] = {"numerator": 1, "denominator": 2}
        self.assertEqual(CPU.compare(self.report, other)["reason"], "unmatched-execution-evidence")

    def test_different_surface_or_ambiguous_execution_cannot_be_promoted(self):
        other = copy.deepcopy(self.report)
        other["conditions"]["extent"] = [640, 480]
        self.assertEqual(CPU.compare(self.report, other)["reason"], "different-observed-conditions")
        self.report["groups"]["ambiguous"]["fraction"] = .2
        self.report["groups"]["P95-bound"]["fraction"] = .79
        self.assertEqual(CPU.compare(self.report, self.report)["reason"], "insufficient-execution-tier-coverage")


if __name__ == "__main__":
    unittest.main()
