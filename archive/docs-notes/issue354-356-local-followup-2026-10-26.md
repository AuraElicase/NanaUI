# Issue #354 / #356 本机跟进（2026-10-26）

> 最终状态：#354 已以触发条件调查和测量归因修正完成，见 [执行上下文证据](issue354-execution-context-2026-10-26.md)；#356 按用户决定以 `NOT_PLANNED` 关闭，未完成的实机矩阵仍是已知限制。父项与各阶段的当前交付范围见 [最终收尾](presentation-output-final-closeout-2026-10-26.md)。下文保留此前各时点的 OPEN 判断、失败和未执行项，不将它们倒填为通过。

基于 `main` 的 `1f91962c744d7c0f3d8c99cf2ae10b94ad0a60dc`，继续前一份 [#242 本机收尾](issue242-closeout-2026-10-26.md)。本机验证结束时改动尚未提交；随后经用户授权使用 CI，已提交并推送到当前 `main`，见下文专项验收跟进。Runtime / Scene 的同期 sticky、layout 及其 fixture 改动不属于本报告的验收输入。

## 本次修复与观察边界

#356 发现真实代码问题：OpenVR / Electron 参考生产端使用保留宿主 callback 的 `GpuContext::from_wgpu`，但自身并未安装或转发 device-lost callback。新增 `from_wgpu_with_loss_tracking` 显式交出 callback 所有权，复用原有共享 loss authority；两个独立生产端改用它。原构造函数继续保留已有宿主 callback，公共 WGPU 签名仍限定在 `wgpu-interop`。

`controlled_device_loss` 在 Metal 上真实执行 `device.destroy()`：验证原构造函数保留宿主通知、跟踪构造函数令所有 clone 观察到粘性丢失、持有旧纹理和提交 ticket 时替换设备、拒绝旧代际纹理，以及新设备上传/提交完成。它不声称模拟一次提交一定还在 GPU 执行，也不是硬件 driver reset、外部消费者 outstanding lease 或两代池上限的实机验收。CI 新增此测试的执行命令，随后已随 `657c0d2` 推送；执行结果由专项运行单独记录。

#354 新增冷路径 `WindowHandle::surface_info()`。由宿主线程返回实际已配置 Surface 的尺寸、格式、色彩空间、present mode、alpha、最大帧延迟、设备代际和当前显示器；窗口代际过期、挂起或恢复未完成明确报错。查询不 acquire、提交、等待或重配。自动模式保持自动身份，不能解释为测得的扫描节拍。

`presentation-output-probe --surface-info` 在首个 direct presented 后查询一次；必须在预热期间获得结果，且查询后直到 direct 测量结束不得 resize/move/mode change。在预热结束和最后一帧采样存储完成后，分别冷读实际 PID、线程 ID 和公开 task category policy。只证明两个端点的身份/角色，不证明进程 uniqueid、完整 TG tenure、coalition 成员关系、CPU 物理驻留或内部角色变化。没有 QoS、affinity 或 core pin 设置。

## 启动对照的失败与修正

本机原始数据位于 `target/issue354-2026-10-26/`；实际启动程序、完整受控环境与进程收据位于本机私有目录，其定位文件为 `target/issue354-2026-10-26/launch-locator.json`。这些路径/环境/进程数据不上传 GitHub。

- 初次短测及加入 PID/TID 后的短测分别保留；不可视作完整五组因果数据。
- `formal/` 首个进程被校验器的环境变量顺序错误拦下。relay 的固定排列顺序为字母序，而校验器按 JSON 插入顺序比较；实际键值完全一致。修正校验器后另开完整序列，未覆盖原始记录。
- `formal-v2/` 第一个 A 与第一个 B 的 Surface、draw、render 和 submission 相同，但上传字节分别为 3,226,734 / 3,226,617，按预先声明的门禁停止。A 测量区间含 1,172 次 prepare-only 尝试；原 fixture 每次 prepare 都递增文本，导致成功呈现的内容与 B 不同。该序列不作为合格的启动时延对照，也没有取前缀拟合。
- 新增显式 `--presented-counter` 诊断选项，文本只在成功呈现后递增，prepare-only 尝试重复内容。默认 fixture 仍按 prepare 次数递增。一次完整长度的双臂预演达到 Surface 与所有 painter 工作计数完全一致，随后冻结新的 binary/helper 开始 `formal-v3/`。这是新诊断 population，不能与旧默认数据相减。

## 冻结协议

A 为直接执行 relay，B 为 LaunchServices 启动同一 relay；两者由同一 relay 使用相同 `posix_spawn` 路径启动相同目标。目标 argv（仅输出路径不同）、全量受控环境、cwd、binary/helper/Info.plist 均冻结；校验每个输入 SHA。父进程原始 task role 只作为观察，不能推断目标 TG/coalition。每次都取得真实 child exit/waitpid 收据，不把 `open` 的返回值等同子进程成功。

五组顺序为 ABBA / BAAB / ABBA / BAAB / ABBA；每进程预热 240 帧、测量 2,400 帧，请求 120 Hz，测量实际跨度必须至少 16 秒。每进程只取成功 presented 帧的 CPU16 与 wall 数据；prepare ordinal 留下未呈现尝试的可见间隙。CPU ≥ 0.9 ms 连续帧计数不叫 E95，也不直接命名任何物理核心类型。CPU 与 wall 不是 GPU execution timer；wall−CPU 也不是纯 GPU/drawable wait。

分析单位为进程分位数，再计算每组 B 均值减 A 均值，最后取五组差的中位数；不把 48,000 个帧样本当成独立进程。源码/构造器测试、默认未插桩时延和 trace 属于各自独立 population。实验期间本任务不启动编译或其他 GPU 测试；同期其他任务的编译/CPU 负载无法控制，逐进程前后记录，不删除受影响结果。

## 尚未满足的关闭条件

#354 仍需在有效调度留存证据中关联实际身份与完整 TG tenure，并定位触发输入或具体源码根因，完成相应最小产品修复与独立验证。启动路线对照即使存在差异，也不能直接解释历史 R1–R5 源码变化。当前 Thread Activity / CLI Deferred 配置已知有丢失，不能提升为有效因果证据；见 `perf/sched_trace/README.md`。本轮等待用户释放至少 12 GiB 后再考虑新 trace，未因空间不足重跑已知必然丢失的配置。

#356 仍需 Windows 硬件 GPU、真实 SteamVR overlay、固定版本 Electron GPU import / 双凭据释放、disconnect/timeout/旧句柄安全和真实驱动恢复完整矩阵。此前 WARP CI 子集保留为实际 API 功能证据；本机 Metal callback 测试不能替代它们。用户确认当前没有这些外部环境，两个 Issue 继续开放。

## 本次启动对照结果

冻结目标 SHA-256：`b14b100f6d5963a88864803a638565ce10928fecde17d96746ef38a99d076d43`。20 个进程全部通过退出/身份/Surface/输出和 painter 工作量检查，测量跨度 19.991–20.066 秒；每进程 2,400 次 render / submit、7,200 次 draw、3,226,617 字节 GPU upload，零额外 copy / CPU readback。配置统一为 480×320、Bgra8UnormSrgb / Srgb、Immediate、PostMultiplied、最大帧延迟 1；显示器上报 1920×1080、scale 1、120,000 mHz。此为配置快照，不是实际 scanout 测量。

| 组 | 顺序 | A CPU p50 均值 ms | B CPU p50 均值 ms | B−A ms |
| --- | --- | --- | --- | --- |
| 1 | ABBA | 0.361146 | 1.068063 | +0.706917 |
| 2 | BAAB | 0.974209 | 0.932416 | -0.041792 |
| 3 | ABBA | 0.468688 | 0.980438 | +0.511750 |
| 4 | BAAB | 0.513771 | 1.229396 | +0.715625 |
| 5 | ABBA | 0.942791 | 0.949770 | +0.006979 |

五组配对差的中位数：CPU p50 +0.511750 ms、p95 −0.098625 ms、p99 −0.170500 ms；wall p50 +0.525396 ms、p95 +0.064021 ms、p99 −0.118250 ms。四组 p50 上升，一组下降，保留全部方向。存在启动归属线索，但后台负载非受控，目标 task role 三次端点发生变化，其他端点还包括角色 1 / 2；不得解释为全程相同角色，角色也不是 TG/core 类型。未取得 TG tenure 或历史源码单变量对照，未发现可据此关闭 #354 的最小产品修复。摘要见 [启动对照 JSON](performance-data/issue354-356-local-2026-10-26/launch-summary.json)。原始数据与全部失败另存本机，摘要移除了实际 PID/TID、环境、路径及 ps 清单。

另外一次独立 `--profile-stages` + `/usr/bin/sample PID 10 1 -mayDie` 诊断成功退出，CPU 中位 0.968625 ms，wall p50 1.067791 ms。阶段 wall 中位：settle 0.193729 ms、encode 0.240791 ms、submit 0.378396 ms、acquire 0.042542 ms、present 0.039188 ms；这不是阶段 CPU 或 GPU duration，也不能把多个阶段的中位数相加当作一帧。实际主线程堆栈含 WGPU render-pass 录制、FrameContext submit、Metal command encoder/commit 和普通事件循环等待；sample 采样覆盖所有 wall 状态，不能由这些函数出现就宣称阻塞点或源码根因。见 [独立诊断摘要](performance-data/issue354-356-local-2026-10-26/stack-summary.json)。

## 验证与 Review

Rust 验证使用本机 `target/issue354-2026-10-26/source/` 的源码导出：原始提交加前一轮 motion 修复及本轮明确列出的改动，不是分支或 Git worktree；`CARGO_TARGET_DIR` 指向原仓库 target，Release 设置 `CARGO_PROFILE_RELEASE_DEBUG=0`。本轮 API / fixture / loss test 的 11 个文件与导出内容核对一致，前一轮 motion 的 3 个实现文件也仍与已测导出一致；完整源文件 SHA 清单、源码导出、二进制、实际 argv 和所有日志保留在本机。未将同期 Runtime / Scene 的 sticky / layout 修改冒充为本轮已测输入。

| 检查 | 实际结果 |
| --- | --- |
| Surface WindowService 单测 | 10 passed，包括新增代际/停止及自动模式身份测试 |
| Metal 实际 controlled_device_loss | 1 passed / 0 ignored |
| nana-ui Release 全量 lib | 完整重跑 975 passed / 1 ignored；此前一次异常退出单列保留，不解释为通过 |
| NanaUI library / tests / presentation fixture clippy | `-D warnings` 通过 |
| controlled loss test clippy | `-D warnings` 通过 |
| Windows GNU library / OpenVR / Web producer check + clippy | 均通过；不代表 Windows 运行或跨 API 像素证据 |
| Electron bridge Node 协议 | 20 passed / 0 ignored；不代表 Electron GPU 运行 |
| fmt、diff、engine/API boundary | 通过；最终格式检查记录随日志保存 |

精确命令（`cargo` 的 manifest 均为上述源码导出的 `Cargo.toml`）：

```bash
WGPU_BACKEND=metal cargo test --locked --release -p nana-ui --features hosted,bundled-fonts --lib window_service::tests -- --test-threads=1
WGPU_BACKEND=metal cargo test --locked --release -p nana-gpu --features wgpu-interop --test controlled_device_loss -- --test-threads=1
WGPU_BACKEND=metal cargo test --locked --release -p nana-ui --features hosted,bundled-fonts --lib -- --test-threads=1
# 上一命令异常退出后，单测重跑通过，再直接运行同一已构建测试程序：
WGPU_BACKEND=metal target/release/deps/nana_ui-45760aab35e99195 --test-threads=1 --nocapture
cargo clippy --locked --release -p nana-ui --features hosted,bundled-fonts --lib --tests --example presentation-output-probe -- -D warnings
cargo clippy --locked --release -p nana-gpu --features wgpu-interop --test controlled_device_loss -- -D warnings
cargo check --locked --target x86_64-pc-windows-gnu -p nana-ui --features hosted,bundled-fonts,native-export,wgpu-interop --lib --example openvr-output-probe --example web-output-producer
cargo clippy --locked --target x86_64-pc-windows-gnu -p nana-ui --features hosted,bundled-fonts,native-export,wgpu-interop --lib --example openvr-output-probe --example web-output-producer -- -D warnings
node --test examples/web-output-bridge/*.test.cjs
cargo fmt --all -- --check
git diff --check
python3 scripts/check-engine-boundary.py
python3 scripts/check-api-convergence.py
```

首次全量在 `overlay_uses_real_gpu_transfer_and_mock_foreign_retirement` 处异常退出（Cargo exit 101，无具体测试失败详情）；该测试单独重跑通过，同一完整测试程序加 `--nocapture` 后退出 0。首次原因未确认，所有日志保留。一次准备重跑的 shell 还明确返回 `can't create temp file ... no space left on device`，该次测试没有启动，不据此推断此前退出的原因。只删除了已完成、未被进程打开且与冻结 binary SHA 相同的本轮标准 example 编译缓存（49,341,504 字节）；冻结证据没有删除。空间曾恢复约 6.8 GiB，收尾检查时仍未达到 12 GiB。

Review 检查了 callback 共享 authority / 原宿主所有权、旧纹理拒绝、新设备完成通知、headless Unsupported / stale WindowClosed 分支、六种 present mode 身份、冷查询位置、未观测值保持 null、默认计数行为不变及同期文件保护。没有新增设备/队列、产品 CPU readback、frame-path I/O 或调度设置。

Instruments 已在运行，但按应用名称两次、按实际 bundle id 一次读取界面均超时；UI 录制的 Immediate 配置尚未验证，未启动新 trace，也没有重跑已知丢失的 CLI Deferred 配置。该失败与空间条件一并保留为下一步输入，两个 Issue 保持开放。

## Issue 同步

#354 与 #356 正文均已更新，并回读确认仍为 OPEN。#354 仅勾选新诊断 population 的实际配置观测，受控负载、完整身份/TG tenure、历史根因和产品修复验收继续开放；#356 记录具体 callback 修复与本机/交叉检查，但实机矩阵不勾选。该次同步发生在源码/报告提交之前；其后提交见下文。最终检查磁盘约 7.0 GiB，尚未达到新 trace 的前提。


## 专项 CI 跟进

用户允许以可信现有证据或 CI 完成验收。已核对 [2026-10-26 绿灯运行 40299654534](https://github.com/sena-nana/NanaUI/actions/runs/40299654534)，其 Windows job 121494121326 的 10 个 native-export 测试无 ignored；真实 native-host 消费 240 次，Microsoft Basic Render Driver（WARP）→同 LUID D3D11，producer CPU readback / explicit CPU fence wait 为 0。实际 GPU copy 为 consumer 240 次 / 884,736,000 字节；GPU execution / driver VRAM 仍为 null。此证据更新了此前 10-23 的子集，不扩展为 SteamVR、Electron 或硬件性能结论。

本轮修复已作为 [`657c0d2`](https://github.com/sena-nana/NanaUI/commit/657c0d2efe4612997ca4966af80891de83bf9d64) 推送到 `main`；以独立 index 仅提交已测源码、证据和工作流，提交前后 80 个工作文件字节保持一致。Runtime / Scene 等其他任务修改仍保留在工作区，没有创建临时分支或 worktree。

新增 `Presentation conformance` 手动工作流并启动 [40318042410](https://github.com/sena-nana/NanaUI/actions/runs/40318042410)，冻结上述提交。Windows Release 下执行真实 controlled-device-destruction / native-export 测试，再串行五轮 native-host 与固定 Electron 44.9.1 的实际 sharedTexture→VideoFrame→WebGPU / 双凭据释放，分别送入维护的 perf extractor 与 invariant gate。缺报告、unsupported、非零退出或超时均不能通过；原始失败产物按 always 上传。YAML 解析及 actionlint 1.7.12 已通过。执行结果见下文。

没有自托管 runner。SteamVR live、硬件 driver reset、硬件性能与当前 Mac 历史调度根因不能由普通 hosted runner 的通过代替。


专项运行 40318042410 已完成，结论为 **failure**。Windows Release 真实 controlled destruction 1 passed / 0 ignored，native-export 10 passed / 0 ignored；第一轮 native-host 240 次实际消费及维护 invariant gate 通过。Electron 44.9.1 实际启动后，`navigator.gpu.requestAdapter({powerPreference: "high-performance"})` 返回空：报告 `status=unsupported`、`reason=No WebGPU adapter`，消费/绘制/提交均为 0。按预先规则中止，没有将 unsupported 写成通过，也没有继续取其余轮次凑满五轮。所有失败产物已保存到该 run 的 `presentation-conformance-windows` artifact。

因此普通 Windows hosted runner 目前只证明了上述 native / callback 子集，尚不能证明 Electron 实际 import / GPU draw / 双凭据释放。新增轻量 `WebGPU capability` 入口：同一版本、本地页与 ANGLE d3d11 配置下分别观察默认请求和标准的显式 fallback 请求，记录 adapter/device 与 feature status，不增加 Chromium 覆盖参数，不导入纹理或声称外部消费通过。分类/实际执行源码的 Node 回归合计 24 passed；WebGPU 选项依据见 fixture README。待轻量运行结果判断该 runner 是否存在可用 fallback device，再决定是否值得重跑完整桥接。


轻量 [40320406836](https://github.com/sena-nana/NanaUI/actions/runs/40320406836) 也已实际完成，结论 failure / capability unsupported：secure context 为 true，WebGPU API 存在；默认 high-performance 与显式 forceFallbackAdapter 请求均返回 null，均未创建设备；Chromium feature status 为 `webgpu=unavailable_software`。这确认当前 hosted Windows runner 无法提供本桥接所需的 WebGPU adapter。没有把软件选择变体、环境不支持或 0 帧消费解释成通过。该结果是平台能力缺口证据，不能判断尚未执行的纹理 import / draw / 双凭据路径是否正确。

后续探测器、24 项 Node 验证和方法文档已提交 [`4103345`](https://github.com/sena-nana/NanaUI/commit/4103345c00faed93aaf9ce12e383a9a27e799f22)。Rust 源码与 657c0d2 完全相同，保留其普通 Rust CI [40318029823](https://github.com/sena-nana/NanaUI/actions/runs/40318029823)；取消了 4103345 自动触发的重复全量 Rust run 40320407528，新诊断已由专属轻量 CI 实际执行。归档时 657 的普通矩阵仍在运行，不写成全量绿灯。运行产物与 SHA 摘要见 [CI 证据 JSON](performance-data/issue354-356-local-2026-10-26/ci-evidence.json)。

当前工程判断：本轮 native / callback 修复已有较强功能证据；#354 的可复现时延差异和 #356 的真实 Electron/SteamVR/硬件恢复尚无足够证据关闭。两个 Issue 保持 OPEN，已完成子项明确列出，不要求为补齐清单而重复运行已证实不具备能力的 runner。
