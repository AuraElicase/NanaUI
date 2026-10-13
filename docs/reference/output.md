# 窗口外输出（ExternalSurface）

`nana_ui::output` 是无窗口呈现（不让操作系统窗口独占渲染目标）的输出边界。启用 `gpu` feature 即可使用，不要求开启 `hosted` 或 `winit`。

普通窗口直接呈现到它自己的 Surface。任何需要消费同一场景的其他目标（如 Spout 发送端、录屏器或嵌套宿主）走**输出（Output）**路径：在窗口本来就要录制的那一帧里，把同一份场景再往 `ExternalSurface` 里画一遍（见下文「窗口输出」）。这里没有拓扑协商器，也没有 CPU 回退路径；需要在另一块设备或另一个进程里拿到像素的消费方，通过原生共享句柄接收。

## ExternalSurface 与 EmbeddedSurfaceNode

`ExternalSurface` 持有一组有界的持久 GPU 纹理，并使用与窗口相同的 `SceneWgpuPainter`。调用 `render` 时会比对 Scene 投影版本、宿主版本、纹理注册表版本、视口尺寸和目标设备代际：

- 若呈现内容未变化，在开始一帧之前直接返回 `Reused`——不分配、不编码、不复制、不提交，也不回读像素。
- 提交后的版本只在队列完成回调触发后才对外发布。
- `sample` 返回最近一次已完成纹理的租约（lease）；drop 租约即归还槽位，不在 CPU 上等待。
- `GpuContext::poll` 与 `ExternalSurface::poll` 只驱动非阻塞完成回调。

`EmbeddedSurfaceNode` 是嵌套消费方的接缝：在父级录制自身场景期间持有子级的 `ExternalFrame` 租约，并可将该租约绑定到父级的 `GpuSubmission`，待父级完成采样后由完成回调回收子级槽位。逻辑与纹理尺寸、设备/资源代际以及内容修订号作为显式元数据传递，父级的变换与裁剪仍留在父级场景中。注意 `HostTextureRegistry` 槽位会持有一份子级句柄克隆，因此不再使用该节点时，父级必须替换或移除该槽位，避免后续子级修订复用目标时旧槽位仍挂在注册表里。

`render` 等价于在表面自有的帧里依次执行 `prepare` → `record` → `bind_submission`。已经在录制帧的宿主可以直接分三步调用：

1. `prepare`：不录制任何命令，直接回答 `Reused`、`Deferred` 或 `Record(plan)`；
2. `record`：用表面自己的 painter（或通过 `ExternalSurface::with_host_painter` 创建时传入的宿主 painter）将 `plan` 的槽位画进调用方的 `FrameContext`。`target_namespace` 将其槽位 ID 与 painter 的其他目标隔开；表面销毁时用 `target_ids()` 从 painter 中移除；
3. `bind_submission`：在该帧的 `GpuSubmission` 上标记槽位在飞（in-flight）。`plan_texture` 即该槽位的纹理，可用于同一帧内追加后续 GPU 工作。

外部目标是同进程、同设备的资源。在 Windows / DX12 下，`native-export` feature 提供 NanaUI 内置的原生跨进程/跨 API 交换（见下文「DX12 共享纹理」）。Linux / Vulkan 侧经评估：wgpu 30 虽可通过 HAL 导入部分 DMA-BUF 图像，但未暴露外部信号量导入/导出及内存导出合同，且队列所有权屏障超出 Nana GPU 的安全合同范围，因此暂不支持原生导出。当消费方在同设备 GPU 上复制输出纹理时，设置 `copy_source`。

生产端必须把未被 `UiScene::projection_revision` 覆盖的 GPU 生产者或宿主纹理修订号一并计入。尺寸变化或设备替换会先创建新资源；若创建失败，旧的已完成表面继续可用。所有目标资源均携带 `DeviceGeneration`，表面遇到不同代际的设备会直接拒绝，不会把过期句柄交给 painter。

输出侧的工作量可通过 `OutputWorkObservation`、表面的 `last_work()` 快照、`WorkCounters` 的 `output_*` 字段以及 `nana_diagnostics::framework::gpu::OUTPUT_*` 指标观测。帧驱动器可用 `record_last_work` 将表面快照并入当帧计数。

## 窗口输出（Window outputs）

一扇窗口可以把自己的场景在离屏输出里再画一份，交给外部消费方（Spout 发送端、录制器）。`RuntimeProgram::window_output(id)` 返回 `Option<WindowOutputConfig>`（`ApplicationState` 则通过 `ApplicationWindow::output` 配置）；宿主每帧读取该配置，返回 `None` 即停止输出。

```rust
fn window_output(&self, id: WindowId) -> Option<WindowOutputConfig> {
    (id == TYPEWRITER).then(|| WindowOutputConfig::default() // 默认 1920x1080, Contain
        .with_extent(WindowOutputExtent::Fixed { width: 1920, height: 1080 })
        .with_export(WindowOutputExport::Native))
}
fn window_output_frame(&mut self, id: WindowId, frame: &WindowOutputFrame, cx: &RuntimeProgramContext<Msg>) {
    // frame.texture(): 同一设备，BGRA8_UNORM，sRGB 编码，预乘 alpha
    // frame.take_native(): 下述 DX12 令牌（Windows，开启 native-export）
}
fn window_output_status(&mut self, id: WindowId, status: WindowOutputStatus, cx: &RuntimeProgramContext<Msg>) {}
```

- **同一场景，同一帧。** 宿主在为该窗口录制的同一个 `FrameContext` 内，于窗口自身绘制之后、`submit` 之前，使用宿主共享的输出配置专用 painter（独立的 `RenderTargetId`、灰度抗锯齿文本、`AlphaEncoding::Gamma`、SDR 参数）把窗口的 `UiScene` 画进输出目标。不会做第二次布局。
- **无变更则零录制。** 输出底层就是 `ExternalSurface`；若投影、宿主纹理、图片和视口均未变化，直接返回 `Reused`，不产出输出帧也不提交。自定义 GPU 渲染器与合成器动画视为每帧变化。
- **隐藏窗口。** 开启 `while_hidden`（默认开启）时，无法呈现到屏幕的窗口（隐藏、最小化、被遮挡）按 `max_fps`（默认 30）节拍运行：宿主排空消息、flush 文档，并在独立帧中只录制该输出（以及窗口的场景生产者）。隐藏的静态窗口什么都不录；尺寸为 0 的窗口不产出帧。
- **尺寸与适配（Size and fit）。** 支持 `Fixed { width, height }`（默认 1920×1080）、`MatchWindow` 或窗口物理尺寸的 `Scale(f)`。painter 做等比缩放，场景不会为输出尺寸重新布局；宽高比不同时按 `Contain`（完整居中，四周透明）或 `Cover`（铺满裁切）放置，不提供非等比拉伸。
- **Alpha 编码。** 支持 `Premultiplied`（gamma 空间，直接匹配 OBS Spout 的「Premultiplied」选项）或 `Straight`（追加一个小型 pass 将颜色除以 alpha 写入独立纹理）。
- **帧生命周期。** `WindowOutputFrame::texture` 从回调起对提交到宿主设备的后续 GPU 工作有效（按队列顺序）；输出在两帧后回收该纹理。全程无 CPU 等待。
- **状态通知。** `WindowOutputStatus` 每次状态变化上报一次：`Active`、`NativeUnavailable`、`NativeDeferred`、`NativeRetired`、`Failed`、`Stopped`。`max_fps` 负责节流；被节流的变更会在到期时产出。

## DX12 共享纹理（`native-export`）

`nana_gpu::NativeExportPool`（需启用 `native-export` feature，仅限 Windows + DX12 设备；对应能力位 `GpuCapability::NativeTextureExport`）持有三张带 `D3D12_HEAP_FLAG_SHARED` 和渲染目标标志创建的 `DXGI_FORMAT_B8G8R8A8_UNORM` 提交纹理（缺少渲染目标标志时 D3D11 拒绝打开共享 D3D12 纹理），每张纹理导出为一个 NT 句柄，并配一把共享围栏（shared fence）。它们在宿主自己的设备上包装为仅 `COPY_DST` 的纹理，不存在第二个设备或队列：

- `stage`：在调用方当帧中录制一次从已渲染纹理到下一个槽位的拷贝；
- `FrameContext::submit`：在提交守卫保护下，紧随 `queue.submit` 之后在设备队列上 signal 当帧的 *ready* 围栏值；
- `finish`：将已提交的拷贝包装为 `NativeFrameToken`。配置了 `WindowOutputExport::Native` 的窗口输出会对每个产出帧执行此流程。

第 `k` 帧（从 0 起算）在围栏值 `2k + 1` 时就绪（ready），在 `2k + 2` 时释放（released）。消费方合同如下：

1. **按 `token.pool_generation()` 打开池（每个代际一次）**：在 `token.adapter_luid()` 指定的适配器上创建 D3D11 设备（`IDXGIFactory4::EnumAdapterByLuid`），对 `slot < NATIVE_EXPORT_SLOTS` 的每个 `token.texture_handle(slot)` 调用 `ID3D11Device1::OpenSharedResource1`，并对 `token.fence_handle()` 调用 `ID3D11Device5::OpenSharedFence`。只要该池还有任何 token 存活，句柄就保持有效；打开后的对象归消费方自己持有。
2. **消费每一帧 token**：先调 `token.accept_release()`，随后在消费方上下文上依次执行 `ID3D11DeviceContext4::Wait(fence, token.ready_value())` → 读取 `textures[token.slot()]`（例如 `CopyResource` 到发送端纹理）→ `Signal(fence, token.release_value())` → `Flush`。
3. **只能在 Wait 之后的 GPU 时间线上 signal release。** 若在生产端的 ready 信号落地前就提前 signal 了 release，生产端信号到达时会把围栏值往回拉，导致池永久挂起（defer）。
4. **未调用 `accept_release` 就直接 drop 的 token** 会由生产端在自己的队列上（排在 ready 信号之后）自动释放；消费方可以直接 drop token 来跳帧。

全程没有 CPU 等待。当消费方尚未释放上一帧时，`stage` 返回 `Deferred(ConsumerBusy)` 且不录制任何命令；窗口输出此时上报 `NativeDeferred`，并继续产出同设备纹理帧。若消费方超过 2 秒未释放，旧池退役并换新池（上报 `NativeRetired`）。尺寸变化、alpha/导出模式切换或设备重建同样会退役旧池，下一枚 token 将携带新的 `pool_generation`。除显式要求 `Straight` 外，像素均为 sRGB 编码、预乘 alpha。导出路径不做任何 CPU 回读；测试与 `window-output-probe` 中的 D3D11 回读仅用于验收核对。

```powershell
cargo test -p nana-gpu --features native-export --test native_export
cargo run -p nana-ui --example window-output-probe --features hosted,bundled-fonts,native-export
```

测试覆盖：导出帧并在同一适配器的 D3D11 设备上打开、逐字节复制比对、未释放帧使下一帧 defer、drop token 自释放、丢弃帧不污染池，以及最后一枚 token 释放后退役池关闭句柄。`window-output-probe` 让一扇透明合成窗口跑过四个阶段（可见/隐藏 × 动画/静态），并在 D3D11 设备上消费每一枚原生 token。

## 参见

- [实时画面](gpu.md) · [窗口](window.md) · [架构合同](architecture.md) · [GPU 与帧生命周期](/learn/gpu-frame)
