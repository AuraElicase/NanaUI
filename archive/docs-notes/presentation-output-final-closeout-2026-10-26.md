# 呈现输出系列与旧 JS 需求收尾（2026-10-26）

用户明确授权：关闭已确认可关闭的 Issue，并完成其余可收尾项。本次仅同步交付范围、证据和 Issue 状态，不改产品代码，不纳入当前工作区并行的 Layout / Inline / Theme 改动。

## 关闭范围

| Issue | 关闭依据 | 原验收中不再作为本阶段阻塞的部分 |
| --- | --- | --- |
| #57 L1 Canvas2D | `NOT_PLANNED`：Vue+JS 作者路径及其 Canvas facade 已删除，原需求不再实施。 | 不把旧 JS Canvas 功能与测试写成已实现；Rust 自绘、资源、文字及 HostTexture 合同继续有效。 |
| #66 structuredClone | `NOT_PLANNED`：承载纯 JS value clone 的内部 JS runtime 已删除。 | 不声称 structuredClone / transfer 测试通过。 |
| #244 Phase 1 | 当前架构、功能和工作量范围交付完成：无窗口输出与 Window 共用 presentation 编排，普通窗口保持 direct Surface。 | 历史重构前后绝对零时延退化未被证明；#354 以触发条件调查和测量归因修正完成，不是产品时延优化通过。 |
| #246 Phase 3 | 当前原生交换实现交付：设备/代际与句柄合同、ready/use/release、GPU transfer、明确 unsupported，Windows native 子集有实际 API 运行证据。 | 不把 Metal/Vulkan native external sharing 写成已支持；完整跨进程、外部 consumer 与硬件恢复矩阵按 #356 的用户决定本阶段不继续。 |
| #248 Phase 5 | 六类 reference fixture、路径/工作量可观测、维护的 output/transport extractor 与 invariant gate 已交付。 | reference 代码存在与本地/mock 门禁通过，不等于 SteamVR/Electron/硬件平台全矩阵通过。 |
| #242 Epic | 按上述当前交付范围完成总收尾；#245/#247/#357 已完成，#354 调查完成，#355/#356 以范围取消或本阶段不再执行关闭。 | 保留各项未知和明确 unsupported，不把关闭子 Issue 等同于原始全部验收已通过。 |

#244/#246/#248/#242 按用户接受的当前交付范围关闭为 `COMPLETED`。旧目标和未完成复选框保留在各 Issue 的历史记录中，当前验收单独列出；没有将未执行的旧项目批量勾选。本次不关闭 #188/#196，也不因删除 JS 作者路径取消 #40/#69 中仍有效的框架需求。

## 当前实现核对

### Phase 1：呈现与 Window 分离

- `nana-ui::output::PresentationDriver` 只要求 `gpu`，消费 `RuntimeDocument` / `UiScene`，不要求 `hosted`、winit 或 OS Window。
- `PresentationFrameDriver` 被 Window 与无窗口路径共用，按 `prepare → encode → finish` 预留、录制及绑定真实 submission。普通 direct 目标使用栈上描述，不创建 retained 输出池或执行 planner。
- `WindowOutputConfig` 的 attach/detach、resize、兼容共享与不兼容分组经过真实 Window fixture；停止输出恢复 direct。
- 现有 `RuntimeApplication` / `RuntimeProgramContext` 等窗口入口保留，迁移入口与限制见 [窗口外输出合同](../../docs/reference/output.md)。公开能力以等价实现为准，不要求重新引入旧 Issue 的 `WindowPresenter` 类名。
- 工作量门禁覆盖 direct 0 extra pass/copy/readback、默认一次提交、静态输出零生产工作。工作量相同不证明任意执行上下文下 CPU 时延完全相同。

### Phase 3：原生交换与同步

- `nana-gpu` 与 frame-exchange/output 边界承载设备/资源身份、代际、所有权及 token；普通控件不持有 native GPU handle。
- Windows DX12 导出经显式 ready / release 值与同适配器 D3D11 Wait/Copy/Signal/Flush 消费；已实现的 export-copy 如实归属 GPU transfer，不冒充直接渲染至共享资源。
- discard、drop/skip、consumer busy、旧代际、retired pool 与资源保活有对应实现和测试。没有 CPU 像素回读/上传产品回退，不用 CPU wait 代替同步。
- 独立 OpenVR / Electron 生产端的 callback 所有权故障由 `657c0d2` 修复，改用 `GpuContext::from_wgpu_with_loss_tracking`；原构造保留宿主 callback 语义。
- `device.destroy()` 只证明受控销毁通知及新代际路径，不证明硬件 driver reset，也不证明实际 foreign outstanding consumer 的完整恢复。

### Phase 5：reference 与验收入口

- Window：`presentation-output-probe`，实际 Surface 与完整六阶段。
- Nested NanaUI：`EmbeddedSurfaceNode`、持久 HostTexture 与逐提交租约，实际像素/几何回归。
- XR：`openvr-output-probe` 与 DX12/D3D11/SteamVR reference，live 环境未验收。
- Native host：`window-output-probe`、`nana-gpu/tests/native_export.rs`，Windows WARP 子集已有实际运行。
- Web：`web-output-producer` 与固定版本 Electron `examples/web-output-bridge`；Node 协议与能力拒绝经过测试，实际 GPU import/绘制/双凭据释放未通过。
- Headless：`PresentationDriver` 与七个 `output-*` GPU 场景；六个 portable transport 使用真实本地 GPU transfer，但 foreign retirement 明确为 mock；另三个 native transport extractor 只接受对应真实消费报告。

## 已有证据与结果边界

| 证据 | 已确认结果 | 不代表什么 |
| --- | --- | --- |
| [本机代码与工作量收尾](issue242-closeout-2026-10-26.md) | 最终 Release lib 973 passed / 1 ignored；五轮 A/B 各 70/70 场景、2,250/2,250 不变量；工作量一致；同帧共享 motion 与 discard 像素回归。 | A 不是历史重构前 baseline；不由这些计数推导零时延退化。 |
| [callback / 平台跟进](issue354-356-local-followup-2026-10-26.md) | 本机完整重跑 975 passed / 1 ignored、Node 协议检查及交叉检查；具体失败与重跑均保留。 | 编译和 Node 协议不是外部平台运行；未知的首次异常原因未被倒填。 |
| [普通 Rust CI 40318029823](https://github.com/sena-nana/NanaUI/actions/runs/40318029823) | `657c0d2` 全部 job 成功，包含本轮 callback / 共享 GPU 写入修复。 | 不验证之后新增的 #354 分析器，也不替代专用 SteamVR/Electron/硬件验收。 |
| [Windows 专项 40318042410](https://github.com/sena-nana/NanaUI/actions/runs/40318042410) | Release controlled destruction 1 passed、native-export 10 passed、首轮 native-host 240 次消费通过。 | 整个 run 为 failure；Electron 无 WebGPU adapter / 零帧，后续轮次未执行，不能称五轮矩阵通过。 |
| [能力探测 40320406836](https://github.com/sena-nana/NanaUI/actions/runs/40320406836) | 默认和显式 fallback adapter 均为 null，确认该 hosted runner 的能力边界；24 项 Node 测试通过。 | `unsupported` 不是消费成功，也不判断未执行的 import/双凭据路径正确性。 |
| [#354 执行上下文调查](issue354-execution-context-2026-10-26.md) | 同一冻结二进制五组、20 进程、48,000 帧；公开计数得到不同的 P 执行时间区间；新增保守归因门禁，14 项测试与完整数值复算通过。 | 不宣称历史源码退化已被修复、产品时延降低或完整 TG/coalition 内部机制已定位。 |
| [当前代码 CI 40331286288](https://github.com/sena-nana/NanaUI/actions/runs/40331286288) | 本次复核时三平台脚本与 packaging、MSRV、Android/OHOS cross job 已成功。 | 完整 run 当时仍执行；没有将尚未结束的 Rust / GPU job 记为通过。 |

仍未知或未验收：实际 SteamVR overlay；Electron sharedTexture/VideoFrame/WebGPU GPU 消费与双凭据释放；硬件 driver reset 与实际 foreign outstanding consumer；硬件五轮性能；GPU duration / driver VRAM 缺失字段；完整跨进程异常矩阵。Metal/Vulkan 无安全合同的 native 交换继续明确 unsupported。后续具备具体设备与复现输入时可重新开启对应平台验证。

## 删除 JS 需求的依据

`e47b57664` 删除 Vue+JS UI 作者路径及其 `nana-ui-vue`、`nana-js-engine`、`nana-js-v8` 和 `packages/nanavue-runtime`。`155cbc3b0` 将文档收敛为 Rust 与 Rust 脚本 `.vue` 方言两种入口。当前 Git 树和 manifests 均无上述 crate/package；[作者入口](../../docs/learn/authoring-paths.md) 描述它们如何在构建期汇入同一 Runtime。

因此 #57/#66 的关闭是原需求失效，不是补写 JS 实现。CSS、Rust 自绘、网络、媒体与其他正式框架合同不因该关闭自动取消。

## 本次验证与变更卫生

- 回读所有目标 Issue 与 #245/#247/#354/#355/#356/#357 的当前状态和关闭原因，再按依赖顺序更新、关闭、回读确认。
- 逐项对照 `docs/reference/output.md`、公开 output 模块、共享 frame driver、native export 测试与 reference 入口；复核已归档报告及 CI 的真实结论。
- 本次只改文档与 Issue 状态；执行所改文件的链接检查、`git diff --check` 和提交范围核对，不重复跑无变化的 GPU / 全量编译。
- 保留旧报告的原始时间、失败、未知指标及未执行清单，仅补充指向本次最终状态的入口。当前工作区其他改动保持原样，不创建分支或 worktree，不将其测试结果写成本次交付。
