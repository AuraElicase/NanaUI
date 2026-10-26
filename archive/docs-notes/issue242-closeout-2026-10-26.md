# Issue #242 收尾核对（2026-10-26）

检查基于 `main` 的 `1f91962c744d7c0f3d8c99cf2ae10b94ad0a60dc`。本记录更新旧交付清单的状态，不把历史构建结果移作当前修改的验收。

## 范围与已取得的证据

- #245（retained / planner / shared render）、#247（同设备嵌套）已关闭。
- #357 于 2026-10-21 关闭：headless 跨 profile 共用一个 painter，仍允许 Window 宿主按 profile 持有 painter。见 [#357 收尾记录](issue357-shared-painter-2026-10-21.md)。
- `521d91ed318234341ce181844dd4e35e7412d233` 已收敛采样合同：consumer 只接收单采样纹理，`RenderTargetRequirements` 不再有 `sample_count`，MSAA 光栅化与 resolve 属于 painter 内部。旧 Issue 中“外部多采样目标尚待实现 / 返回 MultisamplingUnavailable”的描述已过期。外部多采样目标不是当前呈现输出能力承诺；直接 resolve 的进一步优化也不能靠省略必要的线性合成、alpha 或色彩转换实现。
- Windows 已有运行证据：[2026-10-23 main CI](https://github.com/sena-nana/NanaUI/actions/runs/39897222538)，SHA `a601b97c9c93779beb3cf746e9c5e15cdb2a4085`，Windows job `120230338179`。实际 Window 拓扑、native reference ABI 与 DX12 → 同 LUID D3D11 import 门禁通过。native-export 为 10 passed / 0 ignored；native-host 为 30 次预热、240 次消费，`consumer_completed=true`、CPU readback / explicit CPU fence waits 为 0。适配器为 Microsoft Basic Render Driver（WARP），不是硬件 GPU 性能结论。该运行的 SteamVR live case 明确 ignored；Node 协议测试不等于 Electron GPU 运行。GPU duration / driver VRAM 为 null。

## 本次代码收尾

CI 的 headless output 判定循环纳入 `output-incompatible`。原来 benchmark 已生成此场景，但循环只提取 static / multi / nested / conversion / slow / recovery 六项，未判定新场景的 invariant。

共享 painter 的 motion 上传须满足与共享字形图集相同的录制顺序：同帧后一场景的描述符、keyframe 或时钟不得排到前一场景 pass 之前。丢弃帧后，CPU 上传缓存也不能保留仅存在于被丢弃 encoder 中的数据。本次像素回归覆盖不同描述符、同文档不同时间的 A/B/A 绘制，以及丢帧后重试最后场景。

## 仍阻止关闭的验收

| 跟踪 | 必须补齐 |
| --- | --- |
| #354 / #244 | 实际窗口 direct 时延的可比条件、受控对照和因果定位；若涉及产品修复，须复验功能、工作量和时延。工作计数相同不能证明零时延退化。 |
| #356 / #246 / #248 | SteamVR live overlay、固定版本 Electron 实际 GPU import 与双凭据释放、真实 device-loss / 设备替换、跨进程断连与旧资源安全；硬件平台 Release 五轮完整测量。WARP 功能证据只覆盖已执行的子集。 |

Metal / Vulkan native 外部交换没有安全合同的路径继续明确 unsupported。不得用同设备 transfer、mock receipt、resize recovery 或 CI 配置替代上述外部平台与驱动证据。

## 本轮验证

验证范围为原始提交加本次改动的独立源码导出 `target/issue242-closeout/final-source/`，没有 `.git`，未创建分支或 Git worktree。验证途中当前工作区出现并行 sticky / layout 改动；未重置、格式化或修补这些改动，也没有把它们混入 A/B。A、B 在同一个导出路径构建，使用同一 target 缓存；覆盖源码后重新构建并核对二进制 SHA。完整输入 SHA 和二进制身份见 [结果摘要](performance-data/issue242-closeout-2026-10-26/summary.json)。

| 检查 | 结果 |
| --- | --- |
| 最终 Release `nana-ui` lib 全量 | 973 passed / 0 failed / 1 ignored；含 4 项同帧像素测试和 1 项静态初始化预算测试。 |
| 最终 Release Clippy，lib/tests，`-D warnings` | 通过。 |
| engine boundary、API convergence、perf schema/self-test | 在独立导出源码上通过。 |
| 格式与 diff | 初始工作区 `cargo fmt --all -- --check` 通过；最终导出 `cargo fmt -p nana-ui -- --check` 与本次范围的 `git diff --check` 通过。当前混合工作区全量 fmt 因并行 sticky 代码格式未通过，未替其格式化；嵌套导出的全量 fmt 另受 excluded icons crate 向外层 workspace 查找限制。 |
| 五轮 Release 工作量 | A、B 各 70/70 场景门禁、2,250/2,250 不变量。7 个 headless、6 个 portable transport、1 个真实 Window；AB / BA / AB / BA / AB，30 个 GPU fixture 进程串行（extractor 另计）。 |
| 工作量对照 | 已有观测的 render / pass / copy / submit、painter 和测量区间 GPU policy 逐项相同，包括上传字节、写入、拷贝、缓冲重分配与 wait/stall；未观测字段仍保持 null，尤其 direct 的 measurement_gpu_policy 不被解释为零。 |

执行命令（Cargo 命令在独立导出目录执行，`CARGO_TARGET_DIR` 指向原仓库 `target`，`CARGO_PROFILE_RELEASE_DEBUG=0`）：

```bash
WGPU_BACKEND=metal cargo test --locked --release -p nana-ui --features hosted,bundled-fonts --lib -- --test-threads=1
cargo clippy --locked --release -p nana-ui --features hosted,bundled-fonts --lib --tests -- -D warnings
cargo build --locked --release -p nana-ui --features hosted,bundled-fonts --bin nana-output-benchmark --example presentation-output-probe
cargo fmt -p nana-ui -- --check
python3 scripts/check-engine-boundary.py
python3 scripts/check-api-convergence.py
python3 perf/contract.py --check-schema
python3 perf/contract.py --self-test
```

每个 A/B 进程调用维护的 `nana-output-benchmark --output ...`、`nana-output-benchmark --transport-only --output ...` 和 `presentation-output-probe --output ...`；随后对 14 个场景分别执行 `perf/runners/nana/run.py --scenario ... --from-report ... --output ...`，最后 `python3 perf/contract.py --evaluate-invariants ...`。每场景预热 30、测量 240；Window 保留完整六阶段，不使用 direct-only、逐帧 trace 或 profile。具体 argv、返回码、负载记录、完整原始报告和日志保存在本机 `target/issue242-closeout/commands.json` 与 `round-{1..5}/{before,after}/`，不是 GitHub 上的 raw 数据。

A 是本轮 motion 修复之前的代码，不是历史输出重构前的 baseline；因此不能由此关闭 #354。平台为 Apple M4 / Metal。GPU execution duration 和 driver VRAM 未观测，保持 unknown。portable transport 的 foreign retirement 仍为 mock，不升级为外部平台通过。

实际 Window 调用区间墙钟时间（ms；非 GPU 执行时间）：

| 轮次 | A p50 | B p50 | A p95 | B p95 | A p99 | B p99 |
| --- | --- | --- | --- | --- | --- | --- |

| 1 | 0.355250 | 0.310250 | 0.594584 | 0.619083 | 0.784416 | 0.761542 |
| 2 | 0.393833 | 0.382417 | 1.052333 | 0.485041 | 2.901417 | 0.746416 |
| 3 | 0.278250 | 0.324750 | 0.406209 | 0.424417 | 0.756750 | 0.455625 |
| 4 | 0.436917 | 0.370208 | 1.211167 | 0.473333 | 1.747916 | 0.695917 |
| 5 | 0.361542 | 0.245792 | 3.493958 | 0.451166 | 6.454750 | 0.487250 |

保留了第三轮 B 的 p50/p95 上升、其他尾延迟和同臂漂移，不设时延通过结论。Rust heap 在 recovery 的非阻塞完成轮询及部分 Window 测量中有变化，全部保留在摘要；它们统计进程所有 Rust 线程的 allocation requests，不是存活堆、驱动分配或显存，不据此宣称 CPU allocation-free。

首次五轮也完整保留在 `target/issue242-closeout/initial-empty-check/`：它暴露新增空缓存判定使 resize recovery 多上传 368 字节和两次写入/拷贝。本次改成仅在丢帧后设置显式失效标记，新增静态初始化预算回归，再用最终二进制重跑上述五轮；没有覆盖或删除原数据。

## Review 与回归有效性

- 用 A/B/A 三目标区分各文档的 motion 描述符、keyframe 与同文档不同时间；同时覆盖 `paint` 和 `paint_target`，不靠单独 evaluator 证明多目标输出。
- 首次修复反向验证：关闭按序上传时三个像素测试失败；关闭共享资源的 motion 回滚时 discard 测试失败。最终 Release 全量中这些测试及新增的无动画初始化预算均通过；没有将首次反向验证写成最终代码另跑的一组实验。
- 首个 paint 保留原上传批；后续 paint 只上传变化范围，编码器持有暂存资源。共享账本分别记录 atlas / motion，discard 清理 CPU 上传缓存；不新增设备、队列、提交或 CPU wait，不改变公开 GPU ABI。
- 当前工作区同帧 motion 修复的三个实现文件与已测导出源码 SHA 相同。并行 sticky/layout 改动及其引入的测试 fixture 字段没有被移除，也未纳入本次通过结论。无分支、提交或推送操作。

## Issue 同步

#242 / #244–#248 的采样范围与 #357 状态、#356 的 Windows WARP 子集证据已同步到 GitHub。#355 按 `NOT_PLANNED` 关闭，明确是范围取消而非外部多采样实现完成；#242、#244、#246、#248、#354、#356 保持开放。用户确认当前没有外部测试设备，本轮到此完成本机代码与证据收尾，剩余平台矩阵待设备可用后执行。
