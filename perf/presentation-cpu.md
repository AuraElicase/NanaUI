# Real Window CPU attribution

`presentation-output-probe` can observe public macOS execution counters without
changing product scheduling. These options are diagnostic populations and must
not be subtracted from default, stage-profile or trace populations:

```sh
cargo run --locked --release -p nana-ui --example presentation-output-probe \
  --features hosted,bundled-fonts -- \
  --direct-only --timing-samples --presented-counter --surface-info \
  --cpu-endpoints --process-counters \
  --warmup-frames 240 --measured-frames 2400 --output /absolute/path/report.json
python3 perf/presentation_cpu.py /absolute/path/report.json \
  --cpu-map /absolute/path/cpu-map.json \
  --compare /absolute/path/other-report.json
```

The CPU map must come from the measurement host's captured IODeviceTree CPU
nodes (`logical-cpu-id`, `cluster-type`), with string CPU-number keys and a
`cluster_type` field of `P` or `E`. Do not infer core kind from a cluster number,
requested QoS, a parent process, or a different machine's topology. The frozen
M4 map in the Issue 354 evidence folder is only for those reports.

`--cpu-endpoints` reads public `pthread_cpu_number_np` before and after a frame.
These are instantaneous positions, not continuous residency. `--process-counters`
reads public `proc_pid_rusage(RUSAGE_INFO_V6)` outside the thread CPU16 clock
interval. Times are Mach ticks, converted with the captured Mach timebase;
instructions and cycles cover all process threads. A missing API, failed getter
or missing CPU clock makes requested observations incomplete, never a zero.

Both observers can change execution placement. Wall timing includes the ending
queries; CPU16 excludes them. Process counters enclose CPU16 and include other
threads. Neither metric is GPU completion, scanout, or end-to-end latency.

## Conservative execution evidence

Let `C` be a frame's exact main-thread CPU nanoseconds, `T` the enclosing process
CPU delta, and `P` its performance-core CPU delta. Other process threads consume
nonnegative P and non-P time, so main-thread P time is at least `C-(T-P)` and at
most `min(C,P)`. The analyzer uses exact rational Mach conversion and an eight
Mach-tick quantization allowance `G`:

```text
P95-bound if max(0, (C - (T-P) - 2G)/(C+G)) >= .95
E95-bound if max(0, (C - P     - 2G)/(C+G)) >= .95
otherwise ambiguous

aggregate P-time lower = sum(max(0, C-(T-P)-2G)) / sum(C+G)
aggregate P-time upper = min(1, sum(min(C+G,P+G)) / sum(max(0,C-G)))
```

These are sufficient bounds conditional on the OS counter accounting contract,
not exact residency fractions. The allowance handles integer quantization, not
an arbitrary kernel accounting lag. Failed, regressing, incarnation-changing,
unavailable or incoherent counter intervals reject the report. Ambiguous frames
remain in the denominator. Other-thread activity can widen bounds and change
certificate counts without proving a change in the main thread's placement.

Apple's [Recount documentation](https://github.com/apple-oss-distributions/xnu/blob/main/doc/observability/recount.md)
describes per-CPU-kind accounting and the P-only process values. The collector
uses the public SDK ABI; it neither reads private kernel layouts nor controls
QoS, task category, coalition, affinity or frequency.

## Interpretation and exit codes

The comparison rejects different observed surface/work/observer conditions,
disjoint aggregate P-time bounds, unmatched certificate fractions, or more than
10% ambiguous coverage. The default certificate-fraction tolerance is zero:
this is deliberately conservative and can reject otherwise comparable runs.

- Exit 0: observations accepted; comparison, when requested, still needs source
  and execution-context control. This is never a latency or source-cause pass.
- Exit 1: comparison cannot support unstratified latency attribution.
- Exit 2: input rejected, including unavailable or incoherent observations.

Freeze complete source, binary/helper hashes, target arguments/environment/cwd,
content progression, window configuration and workload before a source A/B.
Keep launch route and observers equal. Preserve failures and background-load
observations; do not select a fast prefix or replace an inconvenient run.
Use repeated process-level blocks rather than treating frames as independent
replicates. Neither matching mixtures nor equal instruction counts establish a
source cause. TG/coalition history remains a separate question if that is the
claim being tested.

The [2026-10-26 Issue 354 report](../archive/docs-notes/issue354-execution-context-2026-10-26.md)
contains a same-binary launch intervention, all five group results, public
counter derivatives and a reproducible calculation check.
