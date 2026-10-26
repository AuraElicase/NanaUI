# Issue #242：呈现输出交付与遗留事项

> 最终交付范围及关闭依据见 [2026-10-26 最终收尾](presentation-output-final-closeout-2026-10-26.md)。本页保留旧时点的数据和未验收边界，不以历史 OPEN 状态表示当前状态，也不以本次关闭扩大平台能力。

> 本文的交付计数与未执行边界为 2026-10-15 的历史快照。当前范围、#357 完成状态、单采样 consumer 合同及 Windows WARP 运行证据见 [2026-10-26 收尾核对](issue242-closeout-2026-10-26.md)；不要继续把本文的 #355 / Windows 状态当作当前结论。

2026-10-15。实现范围为 #244–#248 的呈现边界、持久输出、嵌套、原生交换及参考 fixtures；保留单一 `UiWorld → UiScene` 呈现路径、宿主设备/队列和统一 painter 实现。产品禁止 CPU 像素回读回退，无安全 GPU 合同时明确返回不支持。窗口的文本 AA、HDR、分辨率策略没有为共享输出降级。

## 实现与精简

- Window 和 headless 共用 presentation 编排；普通窗口继续 direct Surface，兼容消费方共享 canonical 渲染，不兼容光栅需求明确分组，结构不变时不重算计划。
- 默认三槽、可配置 2–8 槽；一次性 reservation 绑定真实 frame/submission，discard 回滚，拒绝旧代际及跨帧绑定。显示引用与 GPU 提交引用分别保活；慢消费者跳帧而不阻塞 UI。
- 嵌套使用持久 endpoint/HostTexture 和逐提交租约，支持元数据、局部失效、实际 revision 依赖检查、多父消费及 detach。输入 authority 仍属 #243。
- DX12 ready/release 与独立 transport、通用外部 GPU adapter、OpenVR 和 Electron 参考、consumer trace 及 7 类 output / 9 类 transport 性能契约已接入。具体外部平台运行状态见下表。
- 发布前删除两个无行为转发 wrapper 和恒定 Web support API，benchmark 统一使用通用 adapter，删除三处重复 painter 设置；保留全部 reservation、lease、fence、代际和乱序保护。
- 维护 fixture 移除约 1,235 行调查专用实现：native helper、私有执行计数、大快照、focus/category 干预和逐 allocation 模式分支。六阶段、实际 work/painter/heap、trace 故障验收、有界计时和 stage profiling 保留。旧研究参数明确报迁移错误；不是静默换负载。

精简经过交叉独立 review。移除研究代码降低维护与默认路径复杂度，不证明历史 direct 时延问题已经修复。

## 最新源码验证

精确命令、结果、日志 SHA 和新二进制身份见[验证清单](performance-data/issue242-delivery-2026-10-15/validation.json)。以下按命令分别统计，重叠测试不累加为独立 case 总数。

| 命令/范围 | 最新结果 |
| --- | --- |
| `cargo test --workspace --all-targets --locked` | 6,129 passed / 0 failed / 4 ignored，147 个顶层 harness；排除子进程重复计数。 |
| `cargo test --locked -p nana-gpu -p nana-frame-exchange` | 50 passed。 |
| UI 相关 lib/tests/bin/examples，`hosted,bundled-fonts,native-export,wgpu-interop` | 972 passed / 1 ignored；含精简 fixture 的三项有界计时/CLI 测试。 |
| Runtime/Scene lib | 2,291 passed / 1 ignored；feature 与 workspace 命令不同。 |
| `host_texture_paint`，`runtime-agent` | 4/4 真实 GPU 像素通过。 |
| Web bridge Node 协议测试 | 19/19；不等于 Electron GPU 实机。 |
| 相关 crates 全 feature Clippy、workspace Clippy、minimal feature | 通过。 |
| Windows GNU all-targets 类型检查 | 通过；仅编译证据，已有 LiveResize dead-code warnings。 |
| 格式、diff、engine/API/package 边界、perf schema/self-test | 通过。 |
| 仓库门禁自测 | 45/46；既有主题 `design_number=25 > 21` 失败，仍由 [#345](https://github.com/sena-nana/NanaUI/issues/345) 跟踪，没有重录基线。 |
| 新 Release headless/portable 工作量 | 5 轮、10 次进程运行、60/60 门禁、2,030/2,030 不变量通过；每场景预热 30、测量 240。 |
| 新 Release 真实 Window 六阶段 | 未执行：会话预检返回 loginwindow，未创建目标；不继承旧 Window 结果。 |

Release 构建前后 2,427 个源码输入一致，benchmark SHA `4230cc12…fb165`，Window SHA `138745bd…e0758`。GPU 测量由主代理串行执行，未暂停外部应用。六个 headless 与六个 portable transport 场景均有实际本地 GPU 工作；portable foreign retirement 仍是 mock，`actual_platform_import=false`。不将这些结果转换为 direct 时延、Native/SteamVR/Electron 或真实 device-loss 的通过结论。

## Issue 状态与证据边界

| Issue | 交付范围与处理 |
| --- | --- |
| [#245](https://github.com/sena-nana/NanaUI/issues/245) | retained/planner/shared-render 主体收尾；产品不提供 CPU fallback，多采样目标当前明确拒绝，扩展移交 #355。当前源码与相关工作量门禁通过；不兼容 profile 的独立 painter 实例由 #357 继续收口。 |
| [#247](https://github.com/sena-nana/NanaUI/issues/247) | 同设备嵌套、像素/几何、持久租约、局部失效与静态复用收尾；完整输入仍属 #243。当前相关回归与工作量门禁通过。 |
| [#244](https://github.com/sena-nana/NanaUI/issues/244) | 保持开放：direct 零额外 GPU 工作已有证据，零时延退化尚未证明。 |
| [#246](https://github.com/sena-nana/NanaUI/issues/246) / [#248](https://github.com/sena-nana/NanaUI/issues/248) | 保持开放：具体参考代码、协议测试和 CI 已交付，Windows / SteamVR / Electron 与真实外部设备恢复未完成实机验收。 |
| [#242](https://github.com/sena-nana/NanaUI/issues/242) | Epic 保持开放，不把代码交付、配置或交叉编译等同平台与时延验收。 |

独立开单：

- [#354：direct 时延及 P/E 推荐触发](https://github.com/sena-nana/NanaUI/issues/354)。这是未闭环的性能因果调查，不能直接归为平台缺口或噪声。
- [#355：多采样 retained 目标与 resolve](https://github.com/sena-nana/NanaUI/issues/355)。这是代码能力缺口；既有内部 MSAA 几何路径不等于外部多采样输出可用。
- [#356：Native / SteamVR / Electron 实机矩阵](https://github.com/sena-nana/NanaUI/issues/356)。这是具体外部 consumer 的平台验证缺口，通用 GPU 恢复继续由 #275 跟踪。发现代码故障时另列修复。
- [#357：headless 跨 profile 的 painter 实例与资源共享](https://github.com/sena-nana/NanaUI/issues/357)。headless driver 的不兼容组已共用一个 painter，同帧图集顺序、资源计数与五轮对照见[收尾记录](issue357-shared-painter-2026-10-21.md)；Window 宿主仍按 profile 持有 painter。

Metal/Vulkan native 外部共享仍因安全合同未建立明确不支持；同设备 transfer 通过不提升这些能力等级。GPU duration / driver VRAM 未观测时保持 unknown。OpenVR 提交返回、Electron IPC 返回和超时均不是外部 GPU 完成证明。

## 历史结果与保留证据

R3 workspace 曾为 6,126 passed / 4 ignored。R5 修复默认 None 路径 608 字节复制前后，各有独立 100/100 工作量门禁；时延均未判为通过。20 进程源码单变量对照的 p50 差中位 −5.187µs，三降两升，没有稳定收益证据。修复后相对冻结 R4 的另一组默认 p50 配对中位仍 +381.458µs/+25.3629%；不同 population 不能相减解释源码因果。

独立 26 秒 trace 覆盖 2,640 帧和 23 个实际锚点，捕获 61 帧、76 帧持续 E95 慢态，并与目标 thread-group 推荐载荷变化强关联。上游选择输入、归属触发及历史源码原因仍未知。失败的 TCC / loginwindow 启动实验及未完整 capture 未被升级为通过。

[证据摘要](performance-data/issue242-delivery-2026-10-15/evidence-summary.json) 保存配置、结果、冻结二进制 SHA、原始目录定位和完整本地清单的 SHA。清单中约 4.86GB 的现存 trace/XML/报告、源码归档、失败记录、完整研究工具，以及另列的冻结二进制均保留在原工作区，**未上传为 Git 实现内容**；摘要不是这些 raw 文件本体，也不声称 GitHub 可直接取得它们。历史 R1 曾清理约 6.2GiB 的失败 Metal trace，只留下命令/错误记录；该已缺失文件未被列入现存清单，本次没有删除或覆盖现存证据。

[六文件研究源码快照](performance-data/issue242-r5-2026-10-15/research-fixture-source/README.md) 仅供检查/精确重建定位；不自动构建，不成为正式 GPU 合同。现有未跟踪根 `package-lock.json` 不纳入提交，其他工作区改动未重置。
