# Theme audit baseline

`theme-hardcoding.json` is the maintained inventory used by the repository gate.
The reports under `archive/docs-notes/performance-data/` are historical phase
evidence and keep their original source population.

The 2026-10-17 baseline records the source at
`891edf1d985f0bf20867117e28762d5030a851dc`. The September Phase 1 report scanned
131 components; the current tree has 146, including AreaLayout, PropertyGrid,
ScrubField, TimelineEditor and newer feedback controls. Their constructor
defaults, shared geometry, animation helpers and constants changed the inventory:

| Category | September phase report | Current inventory |
| --- | ---: | ---: |
| Component design numbers | 21 | 25 |
| Component motion values | 2 | 3 |
| Shared design numbers | 73 | 78 |
| Shared motion values | 19 | 27 |
| Module constants | 87 | 111 |
| Color literals, component and shared | 0 | 0 |

This records existing debt; it does not remove hardcoding or change product
appearance. The test still rejects growth in component/shared color literals,
design numbers, motion values, and module constants. Prefer moving a value to
the Theme. A population change requiring a new baseline must include a reviewed
inventory diff and its reason; do not rewrite a historical report or update the
baseline merely to hide an unexplained failure.

Regenerate and verify from the repository root:

```sh
python3 scripts/audit-theme-hardcoding.py --output scripts/baselines/theme-hardcoding.json
python3 scripts/audit-theme-hardcoding.py --check scripts/baselines/theme-hardcoding.json
python3 -m unittest discover -s scripts/tests -v
```
