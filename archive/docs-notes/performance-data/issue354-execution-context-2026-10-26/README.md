# Issue 354 numerical evidence

See [the investigation report](../../issue354-execution-context-2026-10-26.md)
and [maintained method](../../../../perf/presentation-cpu.md).

From repository root, with Python 3 and Ruby:

```sh
python3 archive/docs-notes/performance-data/issue354-execution-context-2026-10-26/verify.py
```

This verifies the 20 compressed report derivative hashes, recomputes all 48,000
measured frame classes and exact aggregate execution-time bounds through two
calculation paths, checks every block's p50 delta and exercises the comparison
guard on real matching and mismatching observations. Temporary decompression
uses the system temporary directory and is removed when the check finishes.

`summary.json` contains the frozen protocol, all five block results, every
process summary, original raw/native receipt hashes, and later exact-rational
aggregate bounds and cadence additions. `reports-manifest.json` distinguishes
compressed derivative hashes from original report hashes.

The `g*.json.gz` files retain all numerical observations, including warmup and
ambiguous frames. Native PID/TID snapshots and display identity were removed;
process birth is a constant token. They reproduce the calculations but cannot
revalidate original process identity. Exact identity, native exit, environment,
background-load inventories, failures and original raw reports remain local.

`source-manifest.json` hashes the complete frozen source export. Its compilable
workspace differs from Git `dbfd299dcabb4e2d7a9a2f8a5f721b33c52febe1` only in
the two fixture files changed by this follow-up; both match the committed
versions byte for byte. `frozen-frame-classifier.py` is the exact original
classifier hashed by the protocol. The maintained analyzer subsequently adds
aggregate bounds and stricter report validation; no observation was selected
out or reassigned by that postprocessing.

`relay.c` is the frozen relay source; its generated `relay-settings.h` contains
private paths/environment and remains local. The helper records native spawn,
wait and exit rather than interpreting the launcher's status as target success.

`rusage-calibrate.c` independently compiles against public SDK types with
size/offset assertions and compares Mach-converted process CPU time with the
POSIX process CPU clock. `rusage-calibration.json` is its separate short sanity
check, not a UI performance measurement. `default-six-phase.json` records an
independent uninstrumented-observer default Window behavior run. The test and
clippy logs describe the isolated frozen source build, not concurrent checkout
changes.
