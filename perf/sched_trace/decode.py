#!/usr/bin/env python3
"""Strict decoder and effect gate for the Nana trace-info provider (Issue #359).

The provider (provider/v1/nana-trace-info.instrpkg) records kdebug class 7 /
subclass 2 codes 2 (TRACE_LOST_EVENTS), 5 (TRACE_RETROGRADE_EVENTS) and
6 (TRACE_PAST_EVENTS) verbatim. This module reads `xctrace export` XML for
those tables and states only what the markers prove:

* a marker row means the recorder reported that condition at that time/CPU;
* its four arguments are passed through untouched: they are branch markers,
  not a number of lost events, and nothing here turns them into one;
* no marker rows, or no table, is ``unknown`` - never zero loss.

The effect gate opens only for a natural, whole-run recording with all three
tables present and no markers, checked against a control capture of the same
configuration that did surface a lost marker. Even then only the marker
branches the control showed are proven visible; loss paths that emit no
marker stay unknown. The gate exists for future frozen diagnostics; it cannot
vouch for captures made without v1.

Usage:
  decode.py build  --out DIR
  decode.py record --out DIR --time-limit 10s [--window 1s] [-- load command ...]
  decode.py decode CAPTURE_DIR
  decode.py gate   CAPTURE_DIR --control CONTROL_CAPTURE_DIR   (re-exports both .trace files)

Exit codes: 0 ok / gate open, 1 gate blocked, 2 capture rejected.
"""
from __future__ import annotations

import argparse
import decimal
import hashlib
import json
import os
import re
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

HERE = Path(__file__).resolve().parent
PROVIDER_VERSION = "v1"
PROVIDER_SOURCE = HERE / "provider" / PROVIDER_VERSION / "nana-trace-info.instrpkg"
PROVIDER_MANIFEST = HERE / "provider" / PROVIDER_VERSION / "manifest.json"
INSTRUMENT_TITLE = "Nana Trace Info v1"
TEMPLATE = "System Trace"

KDEBUG_CLASS = 7
KDEBUG_SUBCLASS = 2
# kind -> (schema, kdebug code). SDK sys/kdebug.h: TRACEDBG_CODE(DBG_TRACE_INFO, n).
KINDS = {
    "lost": ("nana-trace-info-v1-lost", 2),
    "retrograde": ("nana-trace-info-v1-retrograde", 5),
    "past": ("nana-trace-info-v1-past", 6),
}
COLUMNS = (
    ("timestamp", "event-time"),
    ("code", "uint32"),
    ("qualifier", "uint32"),
    ("thread", "thread"),
    ("cpu", "core"),
    ("arg1", "uint64"),
    ("arg2", "uint64"),
    ("arg3", "uint64"),
    ("arg4", "uint64"),
)
COLUMN_FIELDS = ["mnemonic", "name", "engineering-type"]
U32_MAX = 2**32 - 1
U64_MAX = 2**64 - 1
QUALIFIER_MAX = 3  # KDBG_FUNC_MASK
# The toc prints the run duration in microseconds; allow that rounding.
DURATION_SLACK_NS = 1_000
DECIMAL = re.compile(r"(0|[1-9][0-9]*)\Z")
DURATION = re.compile(r"(0|[1-9][0-9]*)\.[0-9]{6}\Z")  # how the toc prints seconds
# xctrace prints this end reason when --time-limit elapses; anything else
# (user stop, target exit, failure) is not a natural whole-run end.
NATURAL_END = "Time limit reached"
WHOLE_RUN_MODES = ("Deferred", "Immediate")


class DecodeError(ValueError):
    """The capture does not match the v1 contract; it is rejected as a whole."""


@dataclass(frozen=True)
class Marker:
    timestamp_ns: int
    cpu: Optional[int]
    thread_tid: Optional[int]
    qualifier: int
    args: tuple[int, int, int, int]

    def to_json(self) -> dict:
        return {
            "timestamp_ns": self.timestamp_ns,
            "cpu": self.cpu,
            "thread_tid": self.thread_tid,
            "qualifier": self.qualifier,
            "raw_args": list(self.args),
        }


@dataclass(frozen=True)
class Toc:
    end_reason: str
    recording_mode: str
    instruments_version: str
    template: str
    os_version: str
    model: str
    duration_ns: int
    schemas: frozenset[str]


@dataclass
class KindResult:
    status: str  # "observed" | "unknown"
    reason: str  # "marker-rows" | "no-marker-rows" | "table-absent"
    markers: list[Marker] = field(default_factory=list)

    def to_json(self) -> dict:
        return {
            "status": self.status,
            "reason": self.reason,
            "markers": [m.to_json() for m in self.markers],
        }


@dataclass
class Decoded:
    toc: Toc
    kinds: dict[str, KindResult]

    def to_json(self) -> dict:
        return {
            "provider": PROVIDER_VERSION,
            "recording": {
                "end_reason": self.toc.end_reason,
                "recording_mode": self.toc.recording_mode,
                "instruments_version": self.toc.instruments_version,
                "template": self.toc.template,
            },
            "kinds": {k: r.to_json() for k, r in self.kinds.items()},
        }


def _parse_xml(text: str, what: str) -> ET.Element:
    try:
        return ET.fromstring(text)
    except ET.ParseError as error:
        raise DecodeError(f"{what}: not well-formed XML: {error}") from None


def _single(parent: ET.Element, path: str, what: str) -> ET.Element:
    found = parent.findall(path)
    if len(found) != 1:
        raise DecodeError(f"{what}: expected exactly one <{path}>, found {len(found)}")
    return found[0]


def _text(parent: ET.Element, path: str, what: str) -> str:
    value = (_single(parent, path, what).text or "").strip()
    if not value:
        raise DecodeError(f"{what}: <{path}> is empty")
    return value


def _blank(text: Optional[str]) -> bool:
    return text is None or not text.strip()


def parse_toc(text: str) -> Toc:
    root = _parse_xml(text, "toc")
    if root.tag != "trace-toc":
        raise DecodeError(f"toc: root is <{root.tag}>, not <trace-toc>")
    run = _single(root, "run", "toc")  # appended runs are not supported
    summary = _single(run, "info/summary", "toc")
    device = _single(run, "info/target/device", "toc")
    duration = _text(summary, "duration", "toc")
    if not DURATION.match(duration):
        raise DecodeError(f"toc: duration {duration!r} is not seconds with six decimals")
    duration_ns = int(decimal.Decimal(duration) * 1_000_000_000)
    if duration_ns <= 0:
        raise DecodeError(f"toc: duration {duration!r} is not positive")
    os_version, model = device.get("os-version"), device.get("model")
    if not os_version or not model:
        raise DecodeError("toc: <device> lacks os-version or model")
    schemas = [t.get("schema") for t in run.findall("data/table")]
    return Toc(
        end_reason=_text(summary, "end-reason", "toc"),
        recording_mode=_text(summary, "recording-mode", "toc"),
        instruments_version=_text(summary, "instruments-version", "toc"),
        template=_text(summary, "template-name", "toc"),
        os_version=os_version,
        model=model,
        duration_ns=duration_ns,
        schemas=frozenset(s for s in schemas if s),
    )


def _decimal(value: Optional[str], limit: int, what: str) -> int:
    if value is None or not DECIMAL.match(value):
        raise DecodeError(f"{what}: {value!r} is not a canonical unsigned decimal integer")
    number = int(value)
    if number > limit:
        raise DecodeError(f"{what}: {number} exceeds {limit}")
    return number


def parse_table(text: str, kind: str, duration_ns: int, logical_cpus: int) -> list[Marker]:
    schema_name, code = KINDS[kind]
    what = f"{kind} table"
    root = _parse_xml(text, what)
    if root.tag != "trace-query-result" or [c.tag for c in root] != ["node"]:
        raise DecodeError(f"{what}: expected <trace-query-result> holding exactly one <node>")
    node = root[0]
    if not len(node) or node[0].tag != "schema" or any(c.tag != "row" for c in node[1:]):
        raise DecodeError(f"{what}: <node> must hold one <schema> followed only by <row>s")
    schema = node[0]
    if schema.get("name") != schema_name:
        raise DecodeError(f"{what}: schema {schema.get('name')!r} is not {schema_name!r}")
    columns = []
    for col in schema:
        if col.tag != "col" or [c.tag for c in col] != COLUMN_FIELDS:
            raise DecodeError(f"{what}: <schema> may hold only <col> with {COLUMN_FIELDS}")
        columns.append((_text(col, "mnemonic", what), _text(col, "engineering-type", what)))
    if tuple(columns) != COLUMNS:
        raise DecodeError(f"{what}: columns {tuple(columns)} do not match v1 {COLUMNS}")

    # xctrace writes each distinct value once with id= and points back with ref=.
    defined: dict[str, ET.Element] = {}

    def define(elements: list[ET.Element], where: str) -> None:
        # Only the decoded value and a thread's direct <tid>/<process> get ids;
        # nothing deeper inside <process> is read, so nothing there can be referenced.
        for inner in elements:
            ident = inner.get("id")
            if ident is not None:
                if ident in defined:
                    raise DecodeError(f"{where}: id={ident} defined twice")
                defined[ident] = inner

    def resolve(cell: ET.Element, where: str) -> ET.Element:
        ref = cell.get("ref")
        if ref is None:
            if not set(cell.attrib) <= {"id", "fmt"}:
                raise DecodeError(f"{where}: unexpected attributes {sorted(cell.attrib)}")
            return cell
        if set(cell.attrib) != {"ref"} or len(cell) or cell.text is not None:
            raise DecodeError(f"{where}: ref={ref} cell also carries a value")
        target = defined.get(ref)
        if target is None:
            raise DecodeError(f"{where}: ref={ref} has no earlier id")
        if target.tag != cell.tag:
            raise DecodeError(f"{where}: ref={ref} is <{cell.tag}> but id is <{target.tag}>")
        return target

    def scalar(cell: ET.Element, limit: int, where: str) -> int:
        if len(cell):
            raise DecodeError(f"{where}: a scalar value has child elements")
        return _decimal(cell.text, limit, where)

    markers = []
    for index, row in enumerate(node[1:]):
        where = f"{what} row {index}"
        if row.attrib or not _blank(row.text):
            raise DecodeError(f"{where}: <row> carries attributes or text")
        cells = list(row)
        if len(cells) != len(COLUMNS):
            raise DecodeError(f"{where}: {len(cells)} cells, expected {len(COLUMNS)}")
        values: dict[str, Optional[int]] = {}
        for (mnemonic, engineering_type), cell in zip(COLUMNS, cells):
            at = f"{where} {mnemonic}"
            if cell.tag == "sentinel":
                # xctrace's empty value. Only the thread and CPU may be empty:
                # the kernel synthesizes these markers outside any thread, and
                # real overload captures carry lost markers with no CPU.
                if mnemonic not in ("thread", "cpu"):
                    raise DecodeError(f"{at}: is empty")
                if cell.attrib or len(cell) or cell.text is not None:
                    raise DecodeError(f"{at}: <sentinel> is not empty")
                values[mnemonic] = None
                continue
            if cell.tag != engineering_type:
                raise DecodeError(f"{at}: is <{cell.tag}>, expected <{engineering_type}>")
            fresh = cell.get("ref") is None
            value_cell = resolve(cell, at)
            if mnemonic == "thread":
                if not _blank(value_cell.text) or not {c.tag for c in value_cell} <= {"tid", "process"}:
                    raise DecodeError(f"{at}: <thread> may hold only <tid> and <process>")
                tid = _single(value_cell, "tid", at)
                if tid.get("ref") is None:
                    if not set(tid.attrib) <= {"id", "fmt"}:
                        raise DecodeError(f"{at}: unexpected <tid> attributes")
                    tid_value = scalar(tid, U64_MAX, f"{at} tid")
                    if fresh:
                        define([value_cell, *value_cell], at)
                else:
                    if fresh:
                        define([value_cell, *value_cell], at)
                    tid_value = scalar(resolve(tid, f"{at} tid"), U64_MAX, f"{at} tid")
                values[mnemonic] = tid_value
                continue
            limit = U32_MAX if engineering_type in ("uint32", "core") else U64_MAX
            values[mnemonic] = scalar(value_cell, limit, at)
            if fresh:
                define([value_cell], at)
        if values["code"] != code:
            raise DecodeError(f"{where}: code {values['code']} in the {kind} table (expects {code})")
        if values["qualifier"] > QUALIFIER_MAX:
            raise DecodeError(f"{where}: qualifier {values['qualifier']} exceeds {QUALIFIER_MAX}")
        if values["timestamp"] > duration_ns + DURATION_SLACK_NS:
            raise DecodeError(f"{where}: timestamp {values['timestamp']} is after the run ({duration_ns} ns)")
        if values["cpu"] is not None and values["cpu"] >= logical_cpus:
            raise DecodeError(f"{where}: cpu {values['cpu']} is not below {logical_cpus} logical CPUs")
        markers.append(
            Marker(
                timestamp_ns=values["timestamp"],
                cpu=values["cpu"],
                thread_tid=values["thread"],
                qualifier=values["qualifier"],
                args=(values["arg1"], values["arg2"], values["arg3"], values["arg4"]),
            )
        )
    return markers


def decode(toc_text: str, tables: dict[str, Optional[str]], logical_cpus: int) -> Decoded:
    """`tables` maps each kind to its export XML, or None when it was not exported."""
    if not isinstance(logical_cpus, int) or isinstance(logical_cpus, bool) or logical_cpus <= 0:
        raise DecodeError(f"logical CPU count {logical_cpus!r} is not a positive integer")
    toc = parse_toc(toc_text)
    kinds = {}
    for kind, (schema_name, _) in KINDS.items():
        text = tables.get(kind)
        if schema_name not in toc.schemas:
            if text is not None:
                raise DecodeError(f"{kind}: export present but the toc has no {schema_name}")
            kinds[kind] = KindResult("unknown", "table-absent")
            continue
        if text is None:
            raise DecodeError(f"{kind}: the toc lists {schema_name} but it was not exported")
        markers = parse_table(text, kind, toc.duration_ns, logical_cpus)
        if markers:
            kinds[kind] = KindResult("observed", "marker-rows", markers)
        else:
            kinds[kind] = KindResult("unknown", "no-marker-rows")
    return Decoded(toc, kinds)


def provider_sha256() -> str:
    return hashlib.sha256(PROVIDER_SOURCE.read_bytes()).hexdigest()


def manifest_sha256() -> str:
    return json.loads(PROVIDER_MANIFEST.read_text())["source_sha256"]


def configuration(capture: dict, decoded: Decoded) -> dict:
    return {
        "provider_sha256": capture.get("provider_sha256"),
        "logical_cpus": capture.get("logical_cpus"),
        "instruments_version": decoded.toc.instruments_version,
        "template": decoded.toc.template,
        "recording_mode": decoded.toc.recording_mode,
        "os_version": decoded.toc.os_version,
        "model": decoded.toc.model,
    }


def _recording_problems(capture: dict, decoded: Decoded) -> list[str]:
    problems = []
    if capture.get("provider_sha256") != manifest_sha256():
        problems.append(f"provider {capture.get('provider_sha256')!r} is not the frozen v1 source")
    if capture.get("record_exit_code") != 0:
        problems.append(f"recording exit code {capture.get('record_exit_code')!r}")
    if capture.get("load_command"):
        if capture.get("load_started_after_tracing") is not True:
            problems.append("the load was not started on the tracing-started notification")
        if capture.get("load_exit_code") != 0:
            problems.append(f"load exit code {capture.get('load_exit_code')!r}")
    if decoded.toc.end_reason != NATURAL_END:
        problems.append(f"end reason {decoded.toc.end_reason!r} is not a natural time-limit end")
    if decoded.toc.recording_mode not in WHOLE_RUN_MODES:
        problems.append(f"recording mode {decoded.toc.recording_mode!r} does not keep the whole run")
    return problems


def control_receipt(capture: dict, decoded: Decoded) -> dict:
    """What a control capture proves: these lost-marker branches are visible here."""
    problems = _recording_problems(capture, decoded)
    lost = decoded.kinds["lost"]
    if lost.status != "observed":
        problems.append("it shows no lost marker, so visibility stays unknown")
    elif all(m.cpu is None for m in lost.markers):
        # CPU-less markers are kept, but their origin is unproven; a control needs one with a CPU.
        problems.append("its lost markers all lack a CPU")
    if problems:
        raise DecodeError("control: " + "; ".join(problems))
    branches = sorted({(m.qualifier, *m.args) for m in decoded.kinds["lost"].markers})
    return {
        "configuration": configuration(capture, decoded),
        "visible_lost_branches": [list(b) for b in branches],
    }


def effect_gate(capture: dict, decoded: Decoded, control: Optional[tuple[dict, Decoded]]) -> dict:
    reasons = _recording_problems(capture, decoded)
    for kind, result in decoded.kinds.items():
        if result.status == "observed":
            reasons.append(f"{kind} markers observed")
        elif result.reason != "no-marker-rows":
            reasons.append(f"{kind}: {result.reason}")
    receipt = None
    if control is None:
        reasons.append("no control capture")
    else:
        try:
            receipt = control_receipt(*control)
        except DecodeError as error:
            reasons.append(str(error))
        else:
            for key, value in configuration(capture, decoded).items():
                if receipt["configuration"][key] != value:
                    reasons.append(f"control {key} {receipt['configuration'][key]!r} != capture {value!r}")
    if reasons:
        return {"effect_gate": "blocked", "reasons": reasons}
    return {
        "effect_gate": "open",
        "reasons": [],
        # Only these branches are proven visible; loss without a marker stays unknown.
        "proven_visible_lost_branches": receipt["visible_lost_branches"],
    }


# --- native capture helpers (macOS + Xcode) ---------------------------------


def build(out: Path) -> Path:
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"nana-trace-info-{PROVIDER_VERSION}.instrdst"
    subprocess.run(
        ["xcrun", "instrumentbuilder", "-i", str(PROVIDER_SOURCE), "-o", str(target)],
        check=True,
    )
    return target


def _write_capture(out: Path, capture: dict) -> None:
    (out / "capture.json").write_text(json.dumps(capture, indent=2) + "\n")


def start_on_notification(waiter, tracer, start, poll_seconds: float = 0.02):
    """Start the load only when the notification waiter exits 0 while the tracer runs.

    A waiter that fails (missing tool, bad argument, killed) never starts the load.
    """
    while tracer.poll() is None:
        code = waiter.poll()
        if code is not None:
            return start() if code == 0 else None
        time.sleep(poll_seconds)
    return None


def record(out: Path, time_limit: str, window: Optional[str], command: list[str]) -> int:
    if provider_sha256() != manifest_sha256():
        raise DecodeError("v1 source differs from its manifest; add a new version instead of editing v1")
    out.mkdir(parents=True, exist_ok=False)  # never overwrite an earlier capture
    package = build(out / "package")
    trace = out / "capture.trace"
    notify = f"dev.nanaui.trace-info.started.{os.getpid()}"
    argv = ["xcrun", "xctrace", "record", "--template", TEMPLATE, "--package", str(package),
            "--instrument", INSTRUMENT_TITLE, "--all-processes", "--time-limit", time_limit,
            "--notify-tracing-started", notify, "--no-prompt", "--output", str(trace)]
    if window:
        argv += ["--window", window]
    capture = {
        "provider": PROVIDER_VERSION,
        "provider_sha256": provider_sha256(),
        "logical_cpus": os.cpu_count(),
        "record_argv": argv,
        "record_exit_code": None,
        "load_command": command or None,
        "load_started_after_tracing": None,
        "notify_waiter_exit_code": None,
        "load_exit_code": None,
        "exported": False,
    }
    load = None
    with open(out / "record.log", "wb") as log:
        # The load starts only once xctrace reports tracing, so it all lands in the run.
        waiter = subprocess.Popen(["notifyutil", "-1", notify], stdout=subprocess.DEVNULL) if command else None
        tracer = subprocess.Popen(argv, stdout=log, stderr=subprocess.STDOUT)
        if waiter is not None:
            load = start_on_notification(waiter, tracer, lambda: subprocess.Popen(command))
        code = tracer.wait()
    if waiter is not None:
        if waiter.poll() is None:
            waiter.kill()
        capture["notify_waiter_exit_code"] = waiter.wait()
    if load is not None:
        if code != 0 and load.poll() is None:
            load.terminate()
        capture["load_exit_code"] = load.wait()
    capture["load_started_after_tracing"] = (load is not None) if command else None
    capture["record_exit_code"] = code
    _write_capture(out, capture)  # keep the exit evidence even if export fails
    if code == 0:
        export(out, trace)
        capture["exported"] = True
        _write_capture(out, capture)
    return code


def exported_texts(trace: Path) -> tuple[str, dict[str, str]]:
    """Export the toc and every v1 table the toc lists, straight from the .trace."""
    def run(*extra: str) -> str:
        return subprocess.run(["xcrun", "xctrace", "export", "--input", str(trace), *extra],
                              check=True, capture_output=True, text=True).stdout

    toc = run("--toc")
    present = parse_toc(toc).schemas
    tables = {}
    for kind, (schema_name, _) in KINDS.items():
        if schema_name in present:
            tables[kind] = run("--xpath", f'/trace-toc/run[@number="1"]/data/table[@schema="{schema_name}"]')
    return toc, tables


def export(out: Path, trace: Path) -> None:
    toc, tables = exported_texts(trace)
    (out / "toc.xml").write_text(toc)
    for kind, text in tables.items():
        (out / f"{kind}.xml").write_text(text)


def verify_against_trace(directory: Path) -> list[str]:
    """Re-export from capture.trace and compare with the files the decoder read.

    Table exports are byte-stable; the toc's process list is not, so the toc is
    compared through the fields the decoder uses. This ties a directory to its
    .trace; it does not authenticate the .trace itself.
    """
    trace = directory / "capture.trace"
    if not trace.exists():
        return [f"{directory}: capture.trace is missing"]
    try:
        toc, tables = exported_texts(trace)
        problems = []
        if parse_toc(toc) != parse_toc((directory / "toc.xml").read_text()):
            problems.append(f"{directory}: toc.xml does not match capture.trace")
        for kind in KINDS:
            path = directory / f"{kind}.xml"
            stored = path.read_text() if path.exists() else None
            if stored != tables.get(kind):
                problems.append(f"{directory}: {kind}.xml does not match capture.trace")
        return problems
    except (OSError, subprocess.CalledProcessError, DecodeError) as error:
        return [f"{directory}: cannot re-export capture.trace: {error}"]


def load_capture(directory: Path) -> tuple[dict, Decoded]:
    try:
        capture = json.loads((directory / "capture.json").read_text())
        if not isinstance(capture, dict):
            raise DecodeError(f"{directory}: capture.json is not an object")
        if capture.get("record_exit_code") != 0 or capture.get("exported") is not True:
            raise DecodeError(
                f"{directory}: recording exited {capture.get('record_exit_code')!r} or was not exported; nothing to decode"
            )
        tables = {}
        for kind in KINDS:
            path = directory / f"{kind}.xml"
            tables[kind] = path.read_text() if path.exists() else None
        toc_text = (directory / "toc.xml").read_text()
    except (OSError, json.JSONDecodeError) as error:
        raise DecodeError(f"{directory}: {error}") from None
    return capture, decode(toc_text, tables, capture.get("logical_cpus"))


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("build")
    p.add_argument("--out", type=Path, required=True)
    p = sub.add_parser("record")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--time-limit", required=True)
    p.add_argument("--window")
    p.add_argument("load", nargs=argparse.REMAINDER, help="optional load command after --")
    p = sub.add_parser("decode")
    p.add_argument("capture", type=Path)
    p = sub.add_parser("gate")
    p.add_argument("capture", type=Path)
    p.add_argument("--control", type=Path, help="control capture that surfaced a lost marker")
    args = parser.parse_args(argv)

    if args.command == "build":
        print(build(args.out))
        return 0
    if args.command == "record":
        load = args.load[1:] if args.load[:1] == ["--"] else args.load
        return record(args.out, args.time_limit, args.window, load)
    capture, decoded = load_capture(args.capture)
    if args.command == "decode":
        print(json.dumps(decoded.to_json(), indent=2))
        return 0
    control = None
    tampering = verify_against_trace(args.capture)
    if args.control:
        try:
            control = load_capture(args.control)
        except DecodeError as error:
            print(json.dumps({"effect_gate": "blocked", "reasons": [f"control rejected: {error}"]}, indent=2))
            return 1
        tampering += verify_against_trace(args.control)
    verdict = effect_gate(capture, decoded, control)
    if tampering:
        verdict = {"effect_gate": "blocked", "reasons": tampering + verdict["reasons"]}
    print(json.dumps(verdict, indent=2))
    return 0 if verdict["effect_gate"] == "open" else 1


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except DecodeError as error:
        print(f"rejected: {error}", file=sys.stderr)
        sys.exit(2)
