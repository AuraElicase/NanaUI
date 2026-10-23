import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET

SCHED_TRACE = Path(__file__).resolve().parents[2] / "perf" / "sched_trace"
spec = importlib.util.spec_from_file_location("sched_trace_decode", SCHED_TRACE / "decode.py")
decode = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = decode  # dataclasses resolve annotations through it
spec.loader.exec_module(decode)

DecodeError = decode.DecodeError
# SDK sys/kdebug.h: TRACE_LOST_EVENTS / TRACE_RETROGRADE_EVENTS / TRACE_PAST_EVENTS.
SDK_CODES = {"lost": 2, "retrograde": 5, "past": 6}
CPUS = 10
ALL_SCHEMAS = tuple(s for s, _ in decode.KINDS.values())
WITHOUT_RETROGRADE = (decode.KINDS["lost"][0], decode.KINDS["past"][0])


def toc(schemas=ALL_SCHEMAS, end="Time limit reached", mode="Deferred", version="27.0 (27A266a)",
        template="System Trace", os_version="27.0.1 (26A434)", model="Mac mini", duration="11.209284", runs=1):
    tables = "".join(f'<table schema="{s}"/>' for s in schemas)
    run = (
        f'<run number="1"><info><target><device platform="macOS" model="{model}" os-version="{os_version}"/>'
        f"<all-processes/></target><summary><duration>{duration}</duration><end-reason>{end}</end-reason>"
        f"<instruments-version>{version}</instruments-version><template-name>{template}</template-name>"
        f"<recording-mode>{mode}</recording-mode></summary></info><data>{tables}</data></run>"
    )
    return f'<?xml version="1.0"?>\n<trace-toc>{run * runs}</trace-toc>'


def table(kind, rows=(), columns=decode.COLUMNS, schema=None):
    schema = schema or decode.KINDS[kind][0]
    cols = "".join(
        f"<col><mnemonic>{m}</mnemonic><name>{m.title()}</name><engineering-type>{t}</engineering-type></col>"
        for m, t in columns
    )
    body = "".join(f"<row>{row}</row>\n" for row in rows)
    return (
        '<?xml version="1.0"?>\n<trace-query-result>\n'
        f"<node xpath='//trace-toc[1]/run[1]/data[1]/table[20]'><schema name=\"{schema}\">{cols}</schema>"
        f"{body}</node></trace-query-result>"
    )


def row(code=2, qualifier="0", cpu="7", thread="<sentinel/>", args=("1", "0", "0", "0"), time="569308166", start=1):
    n = iter(range(start, start + 100))
    cells = [
        f'<event-time id="{next(n)}" fmt="00:00.569.308">{time}</event-time>',
        f'<uint32 id="{next(n)}" fmt="{code}">{code}</uint32>',
        f'<uint32 id="{next(n)}" fmt="{qualifier}">{qualifier}</uint32>',
        thread,
        f'<core id="{next(n)}" fmt="CPU {cpu} (P Core)">{cpu}</core>',
    ]
    cells += [f'<uint64 id="{next(n)}" fmt="{a}">{a}</uint64>' for a in args]
    return "".join(cells)


def tables(**overrides):
    result = {kind: table(kind) for kind in decode.KINDS}
    result.update(overrides)
    return result


def json_keys(value):
    if isinstance(value, dict):
        for key, inner in value.items():
            yield key
            yield from json_keys(inner)
    elif isinstance(value, list):
        for inner in value:
            yield from json_keys(inner)


class ProviderSelectorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = decode.PROVIDER_SOURCE.read_bytes()
        cls.package = ET.fromstring(cls.source)

    def test_package_and_instrument_are_versioned(self):
        self.assertEqual(self.package.findtext("id"), "dev.nanaui.trace-info.v1")
        self.assertEqual(self.package.findtext("instrument/title"), decode.INSTRUMENT_TITLE)
        refs = sorted(t.findtext("schema-ref") for t in self.package.findall("instrument/create-table"))
        self.assertEqual(refs, sorted(ALL_SCHEMAS))

    def test_only_three_generated_point_schemas_and_no_derived_layer(self):
        kinds = [child.tag for child in self.package if child.tag not in ("id", "title", "owner", "instrument")]
        self.assertEqual(kinds, ["ktrace-point-schema"] * 3)
        for tag in ("modeler", "import-schema", "point-schema", "interval-schema", "augmentation"):
            self.assertEqual(self.package.findall(f".//{tag}"), [], tag)

    def test_each_pattern_selects_exactly_its_code_and_leaves_qualifier_and_args_open(self):
        by_id = {s.findtext("id"): s for s in self.package.findall("ktrace-point-schema")}
        for kind, (schema_id, code) in decode.KINDS.items():
            self.assertEqual(code, SDK_CODES[kind])
            pattern = by_id[schema_id].find("event-pattern")
            fields = [(child.tag, (child.text or "").strip()) for child in pattern]
            self.assertEqual(
                fields,
                [
                    ("class", "0x7"),
                    ("subclass", "0x2"),
                    ("code", f"?code&{code}"),
                    ("function", "?qualifier"),
                    ("thread", "?thread"),
                    ("CPU", "?cpu"),
                    ("arg1", "?arg1"),
                    ("arg2", "?arg2"),
                    ("arg3", "?arg3"),
                    ("arg4", "?arg4"),
                ],
                kind,
            )

    def test_columns_copy_bound_values_verbatim(self):
        expected = [(m, t) for m, t in decode.COLUMNS if m != "timestamp"]
        for schema in self.package.findall("ktrace-point-schema"):
            columns = schema.findall("column")
            self.assertEqual([(c.findtext("mnemonic"), c.findtext("type")) for c in columns], expected)
            for column in columns:
                self.assertIsNone(column.find("enum"))
                self.assertEqual(column.findtext("expression"), f"?{column.findtext('mnemonic')}")

    def test_manifest_pins_the_source_so_v1_is_not_edited_in_place(self):
        manifest = json.loads(decode.PROVIDER_MANIFEST.read_text())
        self.assertEqual(manifest["source_sha256"], hashlib.sha256(self.source).hexdigest())
        self.assertEqual(manifest["version"], decode.PROVIDER_VERSION)
        self.assertEqual(manifest["selection"]["codes"], SDK_CODES)


class DecoderTests(unittest.TestCase):
    def decode(self, toc_text=None, cpus=CPUS, **overrides):
        return decode.decode(toc_text or toc(), tables(**overrides), cpus)

    def test_marker_is_observed_with_raw_args_and_no_count(self):
        result = self.decode(lost=table("lost", [row()]))
        lost = result.kinds["lost"]
        self.assertEqual(lost.status, "observed")
        self.assertEqual(lost.markers, [decode.Marker(569308166, 7, None, 0, (1, 0, 0, 0))])
        keys = set(json_keys(result.to_json()))
        self.assertFalse({k for k in keys if "count" in k or k in ("lost_events", "dropped", "events_lost")})
        self.assertEqual(result.to_json()["kinds"]["lost"]["markers"][0]["raw_args"], [1, 0, 0, 0])

    def test_refs_resolve_to_earlier_ids(self):
        second = (
            '<event-time id="20" fmt="00:00.914.342">914342583</event-time><uint32 ref="2"/><uint32 ref="3"/>'
            '<sentinel/><core ref="4"/><uint64 ref="5"/><uint64 ref="6"/><uint64 ref="6"/><uint64 ref="6"/>'
        )
        markers = self.decode(lost=table("lost", [row(args=("1", "0", "0", "0")), second])).kinds["lost"].markers
        self.assertEqual(markers[1], decode.Marker(914342583, 7, None, 0, (1, 0, 0, 0)))

    def test_thread_with_tid_is_kept_and_reusable(self):
        thread = '<thread id="90" fmt="t"><tid id="91" fmt="4242">4242</tid><process id="92" fmt="p"/></thread>'
        again = row(start=50, thread='<thread ref="90"/>')
        markers = self.decode(lost=table("lost", [row(thread=thread), again])).kinds["lost"].markers
        self.assertEqual([m.thread_tid for m in markers], [4242, 4242])

    def test_marker_without_cpu_keeps_cpu_unknown(self):
        # Seen in a real overload capture: a lost marker whose CPU is xctrace's empty value.
        no_cpu = row().replace('<core id="4" fmt="CPU 7 (P Core)">7</core>', "<sentinel/>")
        marker = self.decode(lost=table("lost", [no_cpu])).kinds["lost"].markers[0]
        self.assertIsNone(marker.cpu)
        self.assertEqual(marker.args, (1, 0, 0, 0))
        self.assertRejected(rows=[row().replace('<core id="4" fmt="CPU 7 (P Core)">7</core>', '<sentinel fmt="x"/>')])

    def test_empty_or_absent_table_is_unknown_not_zero(self):
        result = self.decode(toc(schemas=WITHOUT_RETROGRADE), retrograde=None)
        self.assertEqual((result.kinds["lost"].status, result.kinds["lost"].reason), ("unknown", "no-marker-rows"))
        self.assertEqual(
            (result.kinds["retrograde"].status, result.kinds["retrograde"].reason), ("unknown", "table-absent")
        )
        for kind in result.kinds.values():
            self.assertIn(kind.status, ("observed", "unknown"))

    def test_toc_and_exports_must_agree(self):
        with self.assertRaises(DecodeError):
            self.decode(lost=None)
        with self.assertRaises(DecodeError):
            self.decode(toc(schemas=WITHOUT_RETROGRADE))

    def test_toc_shape(self):
        bad_durations = ("0.000000", "soon", "NaN", "Infinity", "1e3", "11.2", "-1.000000", "011.209284")
        for bad in (toc(runs=2), toc(model=""), toc().replace("<device", "<x"), *(toc(duration=d) for d in bad_durations)):
            with self.subTest(bad=bad[:80]), self.assertRaises(DecodeError):
                self.decode(bad)
        for cpus in (0, -1, None, True):
            with self.subTest(cpus=cpus), self.assertRaises(DecodeError):
                self.decode(cpus=cpus)

    def assertRejected(self, kind="lost", rows=(), text=None, **table_args):
        with self.assertRaises(DecodeError):
            self.decode(**{kind: text if text is not None else table(kind, rows, **table_args)})

    def test_code_must_match_its_table(self):
        self.assertRejected(rows=[row(code=5)])
        self.assertRejected("past", rows=[row(code=2)])
        self.assertRejected(rows=[row(code=2**32)])

    def test_qualifier_range(self):
        self.decode(lost=table("lost", [row(qualifier="3")]))
        self.assertRejected(rows=[row(qualifier="4")])

    def test_time_and_cpu_are_bounded_by_the_run(self):
        self.decode(lost=table("lost", [row(time="11209284000", cpu=str(CPUS - 1))]))
        self.assertRejected(rows=[row(time="11209286000")])
        self.assertRejected(rows=[row(time=str(2**64 - 1))])
        self.assertRejected(rows=[row(cpu=str(CPUS))])

    def test_argument_range_and_syntax(self):
        top = str(2**64 - 1)
        self.assertEqual(
            self.decode(lost=table("lost", [row(args=(top, "0", "0", "0"))])).kinds["lost"].markers[0].args[0], 2**64 - 1
        )
        for bad in (str(2**64), "-1", "+1", "0x1", "", " ", "1.0", "1e3", " 1", "1\n", "01", "0000001"):
            with self.subTest(arg=bad):
                self.assertRejected(rows=[row(args=("0", "0", bad, "0"))])

    def test_column_set_and_types_are_exact(self):
        columns = list(decode.COLUMNS)
        self.assertRejected(columns=columns + [("arg5", "uint64")])
        self.assertRejected(columns=columns[:-1])
        self.assertRejected(columns=columns[:5] + [("arg1", "kdebug-arg")] + columns[6:])
        self.assertRejected(columns=[columns[1], columns[0]] + columns[2:])
        self.assertRejected(schema="nana-trace-info-v1-past")

    def test_document_shape(self):
        good = table("lost", [row()])
        self.assertRejected(text=good.replace("</node>", "</node><node/>"))
        self.assertRejected(text=good.replace("</schema>", "<note/></schema>"))
        self.assertRejected(text=good.replace("<schema", "<other/><schema"))
        self.assertRejected(text=good.replace("</node>", "<extra/></node>"))
        self.assertRejected(text=good.replace("<row>", '<row hidden="1">'))
        self.assertRejected(text=good.replace("<row>", "<row>junk"))
        self.assertRejected(text="<trace-query-result><node>")

    def test_row_shape_and_cell_types(self):
        good = row()
        core = '<core id="4" fmt="CPU 7 (P Core)">7</core>'
        self.assertRejected(rows=[good[: good.rindex("<uint64")]])
        self.assertRejected(rows=[good.replace(core, '<uint32 id="4">7</uint32>')])
        self.assertRejected(rows=[good.replace('<event-time id="1" fmt="00:00.569.308">569308166</event-time>', "<sentinel/>")])
        self.assertRejected(rows=[row(thread="<sentinel>junk<x/></sentinel>")])
        self.assertRejected(rows=[row(thread='<sentinel fmt="x"/>')])
        self.assertRejected(rows=[row(thread='<thread id="90"></thread>')])
        self.assertRejected(rows=[row(thread='<thread id="90"><tid id="91">1</tid><pid id="92">2</pid></thread>')])
        self.assertRejected(rows=[good.replace('fmt="1">1</uint64>', 'fmt="1">1<uint64 id="99">7</uint64></uint64>')])
        self.assertRejected(rows=[good.replace('<uint32 id="2"', '<uint32 id="2" extra="1"')])

    def test_ids_inside_a_threads_process_cannot_be_referenced(self):
        thread = (
            '<thread id="90"><tid id="91">5</tid>'
            '<process id="92"><uint64 id="93">999</uint64>text<x a="b"/></process></thread>'
        )
        borrow = row(start=50).replace('<uint64 id="54" fmt="1">1</uint64>', '<uint64 ref="93"/>')
        self.assertRejected(rows=[row(thread=thread), borrow])
        reuse = row(start=50, thread='<thread ref="90"/>')
        self.decode(lost=table("lost", [row(thread=thread), reuse]))

    def test_reference_integrity(self):
        first_arg = '<uint64 id="5" fmt="1">1</uint64>'
        self.assertRejected(rows=[row().replace(first_arg, '<uint64 ref="77"/>')])
        self.assertRejected(rows=[row().replace(first_arg, '<uint64 ref="6"/>')])  # forward
        self.assertRejected(rows=[row(), row()])  # same ids defined twice
        core50 = '<core id="53" fmt="CPU 7 (P Core)">7</core>'
        self.assertRejected(rows=[row(), row(start=50).replace(core50, '<core ref="2"/>')])
        self.assertRejected(rows=[row(), row(start=50).replace(core50, '<core ref="4">7</core>')])
        self.assertRejected(rows=[row(), row(start=50).replace(core50, '<core ref="4" fmt="x"/>')])


class GateTests(unittest.TestCase):
    def capture(self, **overrides):
        base = {"provider_sha256": decode.manifest_sha256(), "logical_cpus": CPUS, "record_exit_code": 0,
                "load_command": None, "load_exit_code": None, "exported": True}
        base.update(overrides)
        return base

    def control(self, capture=None, **toc_args):
        decoded = decode.decode(toc(**toc_args), tables(lost=table("lost", [row()])), CPUS)
        return (capture or self.capture(), decoded)

    def gate(self, decoded=None, capture=None, control="default"):
        decoded = decoded or decode.decode(toc(), tables(), CPUS)
        control = self.control() if control == "default" else control
        return decode.effect_gate(capture or self.capture(), decoded, control)

    def test_opens_only_with_a_matching_control_and_a_clean_whole_run(self):
        verdict = self.gate()
        self.assertEqual(verdict["effect_gate"], "open")
        self.assertEqual(verdict["proven_visible_lost_branches"], [[0, 1, 0, 0, 0]])

    def test_blocked_cases(self):
        marked = lambda kind, code: decode.decode(toc(), tables(**{kind: table(kind, [row(code=code)])}), CPUS)
        cases = {
            "no control": dict(control=None),
            "control without lost marker": dict(control=(self.capture(), decode.decode(toc(), tables(), CPUS))),
            "control windowed": dict(control=self.control(mode="Windowed (1 second)")),
            "control stopped early": dict(control=self.control(end="Stopped by user")),
            "control failed load": dict(control=self.control(self.capture(load_command=["x"], load_exit_code=1))),
            "control edited provider": dict(control=self.control(self.capture(provider_sha256="b" * 64))),
            "capture edited provider": dict(capture=self.capture(provider_sha256="b" * 64)),
            "version mismatch": dict(control=self.control(version="26.0")),
            "template mismatch": dict(control=self.control(template="Time Profiler")),
            "os mismatch": dict(control=self.control(os_version="26.4 (25E1)")),
            "model mismatch": dict(control=self.control(model="MacBook Pro")),
            "cpu count mismatch": dict(control=self.control(self.capture(logical_cpus=8))),
            "windowed": dict(decoded=decode.decode(toc(mode="Windowed (1 second)"), tables(), CPUS)),
            "stopped early": dict(decoded=decode.decode(toc(end="Stopped by user"), tables(), CPUS)),
            "nonzero exit": dict(capture=self.capture(record_exit_code=1)),
            "failed load": dict(capture=self.capture(load_command=["x"], load_exit_code=1)),
            "lost observed": dict(decoded=marked("lost", 2)),
            "retrograde observed": dict(decoded=marked("retrograde", 5)),
            "past observed": dict(decoded=marked("past", 6)),
            "table absent": dict(decoded=decode.decode(toc(schemas=WITHOUT_RETROGRADE), tables(retrograde=None), CPUS)),
        }
        for name, kwargs in cases.items():
            with self.subTest(name):
                verdict = self.gate(**kwargs)
                self.assertEqual(verdict["effect_gate"], "blocked")
                self.assertTrue(verdict["reasons"])
                self.assertNotIn("proven_visible_lost_branches", verdict)

    def test_control_needs_a_lost_marker_with_a_cpu(self):
        no_cpu = row().replace('<core id="4" fmt="CPU 7 (P Core)">7</core>', "<sentinel/>")
        control = (self.capture(), decode.decode(toc(), tables(lost=table("lost", [no_cpu])), CPUS))
        self.assertEqual(self.gate(control=control)["effect_gate"], "blocked")

    def test_load_must_start_on_the_tracing_notification(self):
        started = self.capture(load_command=["x"], load_exit_code=0, load_started_after_tracing=True)
        self.assertEqual(self.gate(capture=started, control=self.control(started))["effect_gate"], "open")
        early = self.capture(load_command=["x"], load_exit_code=0, load_started_after_tracing=False)
        self.assertEqual(self.gate(capture=early, control=self.control(started))["effect_gate"], "blocked")

    def test_missing_capture_fields_block_instead_of_crashing(self):
        self.assertEqual(self.gate(capture={"exported": True})["effect_gate"], "blocked")


class CaptureFileTests(unittest.TestCase):
    def test_load_capture_rejects_failed_or_unexported_recordings(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            for capture in ({"record_exit_code": 1, "exported": False}, {"record_exit_code": 0, "exported": False}):
                (directory / "capture.json").write_text(json.dumps(capture))
                with self.assertRaises(DecodeError):
                    decode.load_capture(directory)
            (directory / "capture.json").write_text("[]")
            with self.assertRaises(DecodeError):
                decode.load_capture(directory)
            (directory / "capture.json").unlink()
            with self.assertRaises(DecodeError):
                decode.load_capture(directory)

    def test_verify_against_trace_needs_the_trace(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertTrue(decode.verify_against_trace(Path(tmp)))

    def test_record_refuses_an_existing_directory(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(FileExistsError):
                decode.record(Path(tmp), "1s", None, [])


class FakeProcess:
    def __init__(self, codes):
        self.codes = list(codes)

    def poll(self):
        return self.codes.pop(0) if len(self.codes) > 1 else self.codes[0]


class LoadStartTests(unittest.TestCase):
    def start(self, waiter_codes, tracer_codes):
        started = []
        result = decode.start_on_notification(
            FakeProcess(waiter_codes), FakeProcess(tracer_codes), lambda: started.append(1) or "load", 0
        )
        return result, started

    def test_starts_when_the_notification_arrives(self):
        self.assertEqual(self.start([None, None, 0], [None]), ("load", [1]))

    def test_failed_waiter_never_starts_the_load(self):
        for code in (1, 127, -9):
            with self.subTest(code=code):
                self.assertEqual(self.start([None, code], [None]), (None, []))

    def test_tracer_exiting_first_never_starts_the_load(self):
        self.assertEqual(self.start([None], [None, 1]), (None, []))


if __name__ == "__main__":
    unittest.main()
