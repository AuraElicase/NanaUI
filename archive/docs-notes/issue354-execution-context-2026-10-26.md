# Issue 354: real Window launch context and CPU attribution

2026-10-26. This investigation identifies a reproducible launch-context trigger
for the current direct CPU-time difference and fixes its measurement/attribution
path. It does **not** establish or repair an earlier source-code regression.
No product QoS, core pin, task-category override, extra work, or busy-wait is
introduced. Issue 356 is outside this follow-up.

## Result and disposition

The same frozen executable, same relay, target arguments, complete supplied
environment, working directory, content progression, Surface configuration and
GPU work were run in five process-level blocks. Only entry into the relay
changed: A directly executed it; B opened that same relay through LaunchServices.
Both arms used the same `posix_spawn` to create the actual target.

All five B−A CPU p50 differences were positive. Public performance-core CPU
counters give disjoint main-thread P execution-time bounds for **every** A/B
process pair within each block. This is direct execution accounting, stronger
than inferring hardware execution from TG recommendations or two CPU endpoints.
It demonstrates that launch context is a reproducible trigger for an execution
tier and CPU-time difference in this diagnostic population. It does not isolate
which inherited policy, coalition or kernel decision mediates that trigger, or
prove that placement is its only mediator.

The minimal correction is therefore to retain launch context and actual CPU
execution evidence as comparison conditions and reject unstratified p50 source
attribution when they disagree. `perf/presentation_cpu.py` implements this guard;
the fixture supplies opt-in public observations. No product scheduling workaround
is justified by these results.

Issue 354 can close as a completed investigation and measurement correction under
the user's requested evidence-based closeout. The former full TG-tenure trace
requirement is superseded **for this launch-context conclusion** by enclosing
public execution counters, not marked as collected. Historical source-cause,
kernel TG/coalition mechanism, frequency, thermal effects and cross-platform
latency acceptance remain unknown. Closing does not certify that all possible
context-dependent source regressions are absent or that product latency improved.

## Frozen protocol and all results

- Frozen target SHA-256:
  `aa3bee81845a6ed1efe08cecde92b626c332fecc38b54bbef48d34aef9ea1bef`.
- Relay binary SHA-256:
  `ba8602276ad2fb52de88aa61f318ff755796198834d28a9b8e49af6292d7a91d`.
  Relay source hash:
  `2fa77c89cf4516ffb8cc9f0aebe6f0a67e5bd042a7966f84c75f15aab8b5013d`.
- Orders ABBA / BAAB / ABBA / BAAB / ABBA; 20 fresh processes, each with 240
  warmup and 2,400 measured frames, requested 120 Hz; 48,000 measured frames.
- Flags: `--cpu-endpoints --process-counters --presented-counter --surface-info
  --direct-only --timing-samples --warmup-frames 240 --measured-frames 2400`.
  Only the absolute output filename differed. No trace, sampler or own build ran
  concurrently with this series. Other user processes remained uncontrolled;
  load/process inventories were recorded before and after every process and no
  observations were filtered by load or result.
- Every target's native spawn/wait/exit was successful, PID/TID matched the
  observed target, process birth was stable, observations complete, and no
  configuration changes or missing timing samples occurred. Parent roles are
  observations, not proof of TG or coalition membership.
- Metal Apple M4; actual topology CPU 0–5 E and 6–9 P from IODeviceTree, checked
  against public CPU-number endpoints. Mach timebase 125/3 ns per tick.
- Surface 480×320, Bgra8UnormSrgb / Srgb, Immediate, PostMultiplied, maximum
  frame latency 1. Display reported 1920×1080, scale 1, 120,000 mHz. These are
  configuration values, not measured scanout.
- Each measured run rendered/submitted 2,400 frames, 7,200 draw calls, uploaded
  3,226,617 bytes; no CPU readback, copy, extra pass, GPU buffer reallocation or missing
  painter observation. All content/GPU-work counters matched.

Quantiles use nearest rank; each block compares the mean of its two process
quantiles. Frames are not independent experiment replicates.

| Block | Order | A CPU p50 ms | B CPU p50 ms | B−A ms |
| --- | --- | ---: | ---: | ---: |
| 1 | ABBA | .423021 | .752625 | +.329604 |
| 2 | BAAB | .515854 | .7378545 | +.2220005 |
| 3 | ABBA | .441479 | .717021 | +.275542 |
| 4 | BAAB | .452604 | .803792 | +.351188 |
| 5 | ABBA | .406188 | .7501875 | +.3439995 |

Median paired group CPU p50/p95/p99 differences: +.329604 / +.2827705 /
+.1525835 ms. Wall p50/p95/p99 differences: +.338875 / +.2352295 / +.361 ms.
Wall includes ending observer calls; it is a different endpoint from CPU16.
Across the ten processes in each arm, the median process CPU p50 is .433792 ms
(A) and .7396045 ms (B).

Prepare interval p50 ranges are 8.317250–8.325416 ms (A) and
8.316500–8.328208 ms (B). Per-process p99 ranges are 9.240916–10.148000 ms (A)
and 9.189167–9.791667 ms (B). Each arm has one frame exceeding two requested
intervals; maxima 16.873792 / 17.055625 ms. Measured spans are
19.991434–20.008355 s (A), 19.991047–19.999868 s (B). There were zero
prepare-only gaps between measured presentations. These are callback cadence
observations, not scanout or an assertion that all frame deadlines passed.

## Actual execution evidence and limits

The [maintained method](../../perf/presentation-cpu.md) defines sufficient
main-thread P/E bounds from process-wide counters that enclose the CPU16
interval. Other threads only loosen the bounds. The eight-Mach-tick guard is an
integer-quantization allowance, not an arbitrary kernel accounting-lag guarantee.
The interpretation assumes the public OS accounting fields represent their
documented quantities. Public [Apple Recount documentation](https://github.com/apple-oss-distributions/xnu/blob/main/doc/observability/recount.md)
describes the per-core-kind accounting; the collector uses the SDK's fixed V6
ABI and makes no private layout interpretation.

| Arm | P95-bound frames | E95-bound frames | Ambiguous frames | Total |
| --- | ---: | ---: | ---: | ---: |
| A | 14,636 | 2,063 | 7,301 | 24,000 |
| B | 69 | 23,893 | 38 | 24,000 |

Certificate fractions are sufficient classifications, not exact residency
fractions. The aggregate P CPU-time bounds below account for **all** frames,
including ambiguous ones; intervals in a cell correspond to each arm's two
processes in execution order within that block.

| Block | A main-thread P CPU-time bounds | B main-thread P CPU-time bounds |
| --- | --- | --- |
| 1 | 75.640–88.275%; 93.890–98.603% | 0–.043%; .027–.072% |
| 2 | 21.394–22.031%; 96.956–100% | .534–.754%; 1.410–1.517% |
| 3 | 64.493–69.162%; 89.112–96.035% | 0–.045%; 0–.045% |
| 4 | 75.352–87.523%; 87.433–95.329% | 0–.043%; 0–.039% |
| 5 | 90.275–97.303%; 95.645–99.790% | 0–.044%; .155–.234% |

Every A interval is disjoint from both B intervals in its block. Thus ambiguity
cannot by itself explain the observed difference in execution-time attribution.
This is neither a TG-tenure record nor an observation of CPU frequency.

Requiring at least 60 certified frames per process, all ten A processes have a
P95-bound cohort (median per-process CPU p50 .413334 ms). All ten B processes
have an E95-bound cohort (.741645 ms). Their process-instruction p50 medians
are 1,592,734.5 and 1,597,195.5, about .28% apart. These are descriptive
whole-process instruction counts, not main-thread counts or proof of identical
algorithmic work. B has no process with 60 P95-bound frames, so a matched
P-only A/B performance comparison is **unavailable**, not zero.

The new CLI rejects real g1-1-A / g1-2-B with exit 1 and
`disjoint-execution-time-bounds`. Real g3-2-B / g3-3-B yields exit 0 and
`needs-source-and-context-control`, never a source-cause or latency pass.

## Historical reconciliation

The early +92.3% R1→R2 comparison remains a historical observation of its own
population. The already-published [R6 controlled repeat](issue242-scheduler-controls-2026-10-16.md)
ran frozen R1/R2 at the same executable path, minimal supplied environment and
cwd, without helper/trace. Its five R2−R1 p50 block deltas were +.0016875,
−.325209, −.304646, −.0708135, −.3927705 ms (median −.304646 ms).
Four of five reversed the original direction. This turn re-read that report;
it did not revalidate the historical raw trace or rerun those binaries.

Those results do not establish an absence of conditional source regressions,
but they prevent treating the early increase as an already-proven source
regression. The present same-binary experiment establishes a concrete
non-source comparison confound. Its instrumented values must not be subtracted
from either historical/default or earlier same-binary populations.

## Validation, failed attempts and review

- Independent frozen Git-export sources were built in a separate Cargo target
  directory on the current branch, with all 93 workspace fingerprints removed
  from that directory. Registry dependencies were reused through independent
  APFS COW files. No branch, worktree or reset was created. This excludes the
  concurrent Runtime/Scene/text edits in the main working tree.
- Earlier shared-cache compilation failed after a concurrent schema edit was
  reused against frozen Runtime (`no rules expected identifier inline`). That
  failure and earlier pilots remain local; their binaries are exploratory and
  are excluded from this formal result.
- The first formal controller stopped after one successful native run because
  a local variable shadowed its analysis module. Its complete raw run remains
  local. The corrected controller began a full new 20-process series; no row,
  prefix or replacement run was selected from the failed series.
- Fixture unit tests: 4 passed. Release example clippy with `-D warnings` passed.
  The public-SDK C calibration independently checks size/offsets and CPU Mach
  units against `CLOCK_PROCESS_CPUTIME_ID`; source/output are published.
- Default six-phase real Window run: native exit 0, `passed=true`, all eight
  window-case assertions true, six phase-work entries; both new observers off.
  It covers shared/incompatible rendering, detach, resize and hidden static
  behavior, not a new default latency comparison or pixel review.
- Python focused tests: 10 passed. All 20 public reports / 48,000 measured rows
  reproduce frame classifications and exact rational aggregate bounds through
  Python Fraction and independently expressed Ruby Rational inequalities.
  This is independent calculation, not a second human/agent review. Manual
  code review checked ABI, observation ordering, optional/default behavior,
  rejected/missing observations, interval math and attribution boundaries.
- `cargo fmt --all -- --check` was run and reported concurrent unrelated
  Runtime/Scene formatting differences. Those files were preserved. The two
  changed fixture files pass focused rustfmt; `git diff --check` passes. No
  full-snapshot fmt pass is claimed either: its nested-export metadata lookup
  rejects `nana-icons-tabler` as belonging to the parent workspace.
  No
  claim is made that the entire dirty workspace or full platform suite passed
  in this turn. Ordinary CI runs the Python tests automatically on publication.

Commands for the isolated source export (not the concurrently edited checkout):

```sh
CARGO_TARGET_DIR=/absolute/path/independent-target cargo test --locked --release \
  -p nana-ui --example presentation-output-probe --features hosted,bundled-fonts
CARGO_TARGET_DIR=/absolute/path/independent-target cargo clippy --locked --release \
  -p nana-ui --example presentation-output-probe --features hosted,bundled-fonts -- -D warnings
python3 -m unittest discover -s scripts/tests -p test_presentation_cpu.py -v
python3 archive/docs-notes/performance-data/issue354-execution-context-2026-10-26/verify.py
```

## Evidence retention

[Evidence directory](performance-data/issue354-execution-context-2026-10-26/)
contains all five groups, 20 compressed public report derivatives, original
report/native receipt hashes, CPU map, frozen source manifest and original frame
classifier, relay source, C calibration, test logs, and reproducible Python/Ruby
review. Aggregate bounds/cadence and public export were postprocessing additions;
the original classifier/protocol was frozen before the formal series. No
measured frame was removed or reclassified by the later analysis additions.

Public derivatives remove native PID/TID and display identity and replace process
birth with a constant token. All numerical counters/timings remain unchanged.
The published calculation check cannot re-establish original native identity.
Full source export, frozen binaries, exact helper settings/environment, process
identity/native receipts, load inventories, raw reports, controller failures and
logs remain under local `target/issue354-*` and the user's private diagnostics
directory `nanaui-issue354-core-formal-2026-10-26`. The manifest records original
hashes separately from redacted derivative hashes.
