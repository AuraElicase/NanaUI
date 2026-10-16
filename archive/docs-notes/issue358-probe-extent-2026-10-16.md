# Issue #358：direct-only 首帧缓存几何

`presentation-output-probe --direct-only` 在第 0 阶段结束，而原来的 `original_extent` 只在第 4 阶段请求 resize 前填充。因此已经呈现的 direct-only 报告仍输出 `[0,0]`。本次修复在首个 direct presented callback 读取已有的 `WindowGeometry.physical_size`，并增加 `original_extent_scope`；完整六阶段仍在 resize 前刷新原 baseline。是否已观测由实际呈现计数判断，零尺寸不充当“未呈现”哨兵。

实现只有 example 的 12 行，不新增原生查询、纹理、GPU pass/copy/submit/readback、布局或调度策略。这个字段是**回调中的缓存窗口几何**，不能当作 acquired texture 尺寸、连续逐帧尺寸或实际 present mode。没有产品 direct p50 根因修复声明；#354 保持开放。

## 实测与性能边界

冻结二进制 A/B 的记录构建 HEAD 为 `3a37b9490ecc070fad7b9a9670afe7518bbf7b13`，源码分别为该 HEAD 的 example 与增加上述 12 行的版本。相同显式 Release 构建命令、features、target-dir 已记录；当时没有完整依赖源码及继承编译环境快照，所以不宣称全部代码生成条件一致。实际比较始终使用保存后的不可变二进制 SHA。

- direct-only 两进程：A 的 `[0,0]` 缺口复现；B 为 `[480,320]`，scope 为 `cached WindowGeometry at first direct presented frame`。两边各 30 帧预热、240 帧测量，完整 16 项工作量及 painter 相同，各 240 render/submit、720 draw，额外 GPU 工作为 0；七项 Rust heap 均有实际观测。此两进程只验功能，不作时间对照。
- 完整六阶段：ABBA / BAAB / ABBA / BAAB / ABBA 共 20 进程，无 helper、observer 或 trace。每个进程完整通过 direct、compatible、incompatible、detach、resize、hidden-static 的功能和工作量门禁，缺指标不补零。producer 静态阶段已观测 render/copy/resolve/submit 为 0；这里没有静态阶段 painter upload 观测，不能把缺失 upload 指标当作 0。
- consumer trace 与 trace failure recovery 默认未启用；报告中的条件布尔值为 true 不等于执行过这两项测试，明确记作 `not_executed`。

五组 B−A 的 process p50 配对均值差如下，完整 p50/p95/p99、两侧同臂漂移及每次 heap 计数见[可移植摘要](performance-data/issue242-r6-2026-10-16/portable-extent-results-v1.json)：

| block | 顺序 | p50 差值 ms |
|---|---|---:|
| 1 | ABBA | −0.0199790 |
| 2 | BAAB | −0.0717920 |
| 3 | ABBA | −0.0310215 |
| 4 | BAAB | −0.0728120 |
| 5 | ABBA | +0.2970205 |

第五组正值及首组 p99 波动均保留，没有替换、裁剪或合并帧充当重复。这里是 prepare 到 presented 回调的 wall time，未取得 GPU timestamp；时间只作二进制 population 的描述性关联，**没有硬通过时间回归门禁**。实际最终环境、连续前台、连续 direct 物理尺寸、实际 present mode、P/E/TG 和 thermal 状态未取得；用户允许保持当前外部负载，没有暂停其它程序。额外 GPU 工作的不变量与时间验收分开。

采集期间发现远端及本地 main 已增加他方提交 `11c24c388810e503464601a6bc264a1966233668`（合成层首帧修复）。上面的冻结 population 不冒充新 HEAD 的证据。新 HEAD 已单独重建并执行 example 测试 3/3，随后对全部 tracked regular files（含 shader、asset、build script）做前后哈希一致的 up-to-date build 验证；编译器/sysroot/Cargo home 仍不是完整可复现构建证明。新二进制为 `4b1571e52a0c5939b235895dcf44a1c1cbebd58a925354d6fa127766cd6bb8e8`。当前 HEAD 的完整六阶段与 direct-only 各一个实际窗口进程均 exit 0，分别验证 resize 前缓存 scope 和首个 direct presented 缓存 scope；运行输入前后 SHA 一致。它们仅验兼容性，不与旧 20 进程时间混合。

采后独审又观察到工作区的他方 Runtime 编辑，涉及 `overlay_surfaces.rs`、`world/motion.rs` 和 Runtime 文档。上述验收绑定保存的构建输入及不可变二进制，不覆盖这些后续编辑；交付只选择本项 example 补丁和证据文档，保留其它改动。

## 验证命令

独立 reviewer 从原始报告复算了全部 20 个六阶段进程、两个 direct-only 对照及新 HEAD 的两个兼容进程，逐项复验工作量、七项 heap、painter、运行次序、输入/产物 SHA 和全部时延差值。记录为 `review/extent-native-control-v2-actual-review.json`、`review/probe-extent-direct-actual-review.json`、`review/probe-extent-current-actual-review.json` 及 `review/probe-extent-build-provenance-limitation-supplement.json`，摘要绑定其 SHA。结论为限定的功能/工作量通过，时间与 source-only 因果不予通过。最初 scope 零值哨兵、适配器字段及指纹检查、采前脚本误判等失败与修复记录均保留，没有替换成成功记录。

```sh
cargo test --locked --release -p nana-ui --example presentation-output-probe --features hosted,bundled-fonts --target-dir target/issue242-r6-probe-extent --jobs 2
cargo build --locked --release -p nana-ui --example presentation-output-probe --features hosted,bundled-fonts --target-dir target/issue242-r6-probe-extent --jobs 2
cargo fmt --all -- --check
git diff --check
python3 scripts/check-engine-boundary.py
python3 scripts/check-api-convergence.py
python3 perf/contract.py --check-schema
python3 perf/contract.py --self-test
```

窗口采集入口及定义、输入前后 SHA、原始 report/stdout/stderr、失败记录和独审均在 `performance-data/issue242-r6-2026-10-16/` 本地保留，摘要列出相对路径、大小及 SHA；大文件未加入 Git。Windows、Native/OpenVR、Web bridge 与真实 device-loss 的平台验收没有因本项而通过。workspace 全测试和 Clippy 未在这次 12 行诊断修复中重跑。
