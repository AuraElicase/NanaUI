# 实时画面与 GPU 集成合同

本文规定 NanaUI 的宿主 GPU 上下文（`GpuContext` / `FrameContext` / `GpuTexture`）、实时纹理挂载（`HostTexture` / `GpuTextureView`）、同 pass 直写（`GpuView`）、跨线程最新帧交换（`FrameExchange`）以及按图离屏生产（`SceneResourceProducer`）的完整技术合同。

关于无窗口的保留输出、目标规划与多消费方（如 Spout / 录屏器）路径，请参见 [窗口外输出（ExternalSurface）](output.md)；本文所述的 `GpuContext`、`FrameContext` 与 `GpuTexture` 同样是该输出边界的唯一设备、提交与资源所有者。

## 后端选择、GPU 策略与 HDR 呈现

### 1. Native RHI 决策门（Issue #186）与 WGPU 后端自选

Issue #186 的评估结论为 **NO-GO**：WGPU 仍是 NanaUI 的唯一正式图形后端，正式路径保持 `Logical GPU ABI -> WgpuBackend -> wgpu`（详见 [Issue #186 交付记录](../../archive/docs-notes/consumer-upgrade-2026-09-25-issue186.md)）。

框架 crate 不会越俎代庖地替应用锁死图形后端：workspace 的 `wgpu` 默认只开启 `std`、`parking_lot` 与 `wgsl`；`nana-ui` 的默认特性 `wgpu-backends` 在启用 `gpu` 时会打开当前平台的全部标准后端（Windows 为 DX12 / Vulkan / GLES，Apple 为 Metal，Linux 为 Vulkan / GLES）。如果需要缩减体积、只发行特定图形后端，请在依赖中设置 `default-features = false`，并在应用自己的 `wgpu` 依赖上按平台显式列出所需后端——`hosted_context` 只会在实际编入的后端中选择适配器。DX12 下着色器编译器（`dxcompiler.dll` vs FXC）的选择规则见 [两阶段启动 · DX12 着色器编译器](startup.md#dx12-着色器编译器)。

### 适配器选择

宿主只在能呈现主窗口的适配器中选择，选择在启动线程完成，设备丢失后的替换设备按同一规则重选：

- 应用经 `StartupOptions::with_adapter(GpuAdapterChoice)` 指定。`Matching(GpuAdapterId)` 按 vendor／device id 匹配，同型号多卡再比 PCI 总线地址，任一方没有总线地址时比名称；该卡不在、不能呈现主窗口或建不出设备时退回自动选择，不算启动失败。
- `Automatic`（默认）沿用 wgpu `request_adapter` 的规则：设置了 `WGPU_ADAPTER_NAME` 时取名称包含它的第一块，否则按 `WGPU_POWER_PREF` 稳定排序后取第一块；都未设置时就是枚举顺序的第一块。应用的 `Matching` 优先于这两个环境变量。
- 程序在 `RuntimeProgramContext::gpu_adapters()` 读到本次设备创建时的 `GpuAdapterSelection`：可呈现主窗口的适配器（枚举顺序）、实际所用的一块，以及所指定的卡是否被跳过（`requested_missing`）。没有替宿主选择设备的上下文（headless、staged、嵌入式注入）返回空列表或只含注入设备一项。设置界面应读这份值，不再自行枚举适配器。
- 运行中不切换适配器：应用保存新的选择，下次启动生效。

### 2. 统一 GPU 策略（Issue #184）

`GpuContext::policy()` 按设备代际（`DeviceGeneration`）维护共享的管线注册表（pipeline registry）与工作计数。`SceneWgpuPainter` 的静态管线通过它跨窗口复用底层 WGPU 对象；`GpuWorkSink` 将缓冲上传与重分配统一记入设备统计（`GpuPolicyStats`：含 `upload_bytes`、`upload_writes`、`upload_copies`、`upload_flushes`、`upload_ring_allocations`、`upload_ring_waits`、`frame_slot_waits`、`frame_slot_stalls` 等），公开边界始终保持为纯 `nana-gpu` 类型。

### 3. HDR 显示信息与色彩空间合同（Issue #283）

Surface 的 HDR 能力由 `SurfaceCapabilities::format_capabilities` 决定，而 `wgpu::Surface::display_hdr_info` 返回的是当前显示器的运行时建议值（仅用于调节最终的色调映射 tone-map：Apple EDR 提供相对 headroom，Windows 提供峰值亮度与 SDR 参考白 nit 值，缺失仅代表未知而非 SDR，不能单凭它决定是否开启 HDR Surface）：

- **事件驱动刷新**：宿主在创建 Surface、设备恢复以及窗口移动、缩放、resize 等显示事件中缓存并更新 `DisplayHdrInfo`（稳态帧不轮询操作系统，Metal 查询始终在事件线程执行）。数值未变时不触发重绘；数值变化时仅更新呈现 uniform，不重建 Runtime 或 Scene 状态。安全 headroom 取值为 `DisplayHdrInfo::tone_map_headroom().unwrap_or(1.0)`，SDR 参考白缺省按 `80 nit` scRGB 约定处理。嵌入式宿主收到系统显示通知时，可在窗口线程调用 `EmbeddedRuntime::refresh_display_hdr_info()`（或 `HostedGpuContext` 的同名方法）。
- **线性 scRGB 工作空间**：`SceneWgpuPainter` 的内部合成工作空间统一为**预乘线性 scRGB（premultiplied linear scRGB）**。图表标记（`ScenePrimitiveKind::Chart`）由折线、面积、形状三条解析式抗锯齿管线绘制（边缘为 1 设备像素过渡，不依赖 MSAA），顶点与样式数组按 revision 常驻于目标 GPU 缓冲，首次遇到图表时才惰性编译管线。
- **跨线程与离屏导出的色彩元数据**：`FrameExchange` 仅按字节复制纹素而不做色彩空间转换。生产端需要跨线程传递色彩空间时，应调用 `copy_from_with_metadata` 附带 `FrameColorMetadata`（声明原色 primaries、传递函数 transfer 与 alpha 语义），消费端从 `FrameLease::color_metadata()` 或 `FrameBinding::color_metadata()` 读取（旧 `copy_from` 标记为 `Unknown`，消费端严禁将其静默假定为 sRGB）。`FrameBinding` 默认只直接接收全范围 sRGB 或 80-nit 线性 scRGB；已声明的 P3、PQ、HLG、有限范围或非预乘 alpha 帧会留在收件箱中不被误采样，需由生产端先行转换或由自定义消费端处理。
- **`nana-ui-devtools::offscreen` 快照边界**：固定使用 `BGRA8_UNORM_SRGB`。导出 PNG 应使用 `readback_image` / `paint_image` 返回的 `SnapshotImage` 并调用 `into_png_srgb` 或 `write_png_with_metadata`（自动完成预乘 sRGB 到直通 alpha sRGB 的转换并写入 PNG sRGB 块）。

### 4. 帧上传环（UploadRing）、帧槽（Frame Slots）与缓存

- **批量合并上传（`UploadRing`）**：渲染器不散乱调用 `queue.write_buffer` / `write_texture`，而是通过 `GpuWorkSink` 将写入追加到当前 `FrameContext` 的上传批。调用 `FrameContext::submit` 时，设备待落地批（含帧外写入与此前被丢弃帧的写入）与本帧上传批通过一次 `memcpy` 拷入 `UploadRing`（上限 `32 MiB`）的一个映射块，合并为单个上传命令缓冲并在同一次 `queue.submit` 中排在渲染命令之前落地，从而消除每次小写入在 WGPU 内部的 staging 分配（文本字形块仍走 encoder 内按录制位置的 `copy_buffer_to_buffer`，以保证单次提交前同一目标多次绘制各自看到正确的字形，见 #224；同一帧内第二个及之后的 paint 写入的字形图集区域同样按录制位置拷贝，见下文“同帧多目标的图集顺序”，计入 `upload_bytes` 但不计入 `upload_writes` / `upload_copies`）。
- **有界帧槽（Frame Slots）**：每个 `FrameContext` 在录制期占用一个帧槽（discard 立即归还，submit 在 GPU 完成回调中归还）。槽满时 `begin_frame()` 对最早已提交的帧做一次有界等待；`try_begin_frame()` 则在槽满时立即返回 `None`。窗口呈现路径使用 `try_begin_frame()`，且**必须在拿到帧槽之后才调用 `get_current_texture`**（拿不到槽时先非阻塞 `poll` 一次，仍满则直接跳过本帧，让已上屏的缓冲继续留在屏幕上，避免透明交换链取走缓冲后等待导致整窗空白）。
- **代际化缓存与 URL 纹理共享**：管线与资源布局注册表采用按 `DeviceGeneration` 隔离的 `StampedCache`（满时线性淘汰最久未用的 1/8，并在锁外并行编译管线）。`SceneWgpuPainter` 持有唯一的 `UrlTextureCache`，供 Quad 背景、`border-image` 与 HostTexture 遮罩共享（最多并发拉取 4 个 HTTP 资源，闲置上限 256 项 / `64 MiB`，连续 120 个绘制帧未引用后自动释放；栅格图解码上限 `4096 px` / `64 MiB`，SVG 上限 `2048 px`）。
- **跨 profile 共享 painter 核心**：headless `PresentationDriver` 的不兼容输出组共用一个 `SceneWgpuPainter`。与 profile 无关的部分只存一份：文本引擎、shape / raster 缓存与字形图集、`UrlTextureCache` 与 motion 资源；图标图集属于目标状态，每个绘制过的 `RenderTargetId` 各一份（另有 painter 自身默认目标的一份）。每个工作格式（及其 URL 源编码）保留一份管线集（quad、mesh、图标与文本程序、HostTexture、backdrop、chart），切换 profile 时换入，首次使用时构建。URL 纹理按 egress 与源编码（sRGB 或 legacy 线性直写）分桶，两种编码互不覆盖。每个目标终身只用一个 profile；共享图集为任一 profile 淘汰或搬移字形都会推进 placement epoch，使其他目标保留的 `PreparedBatch` 重建。窗口宿主仍按 profile 各持有 painter。
- **同帧多目标的图集顺序**：一个 `FrameContext` 的上传批在提交时整体排在该帧所有 pass 之前，而字形图集被该 painter 绘制的所有目标共享。因此同一帧内第二个及之后的 paint 把字形区域改为在该帧 encoder 的录制位置上 `copy_buffer_to_texture`，排在此前已录制的 pass 之后，后一组淘汰、复用或 compact 前一组的字形矩形时，前一组仍采样自己的字形。首个 paint 仍走上传批，单目标帧成本不变。窗口与它的同 profile 输出在一帧内共用 painter 时同样适用。这些拷贝随帧的 encoder 存在：帧被丢弃时它们没有落地，painter 在下一次绘制前让图集忘掉全部字形位置，各目标重新放置并重建批次。一个 painter 同一时刻只向一个帧录制：宿主提交或丢弃当前帧之后再向下一帧绘制，乱序提交的两个帧会让一帧的图集写入落在另一帧的 pass 之下。
- **同帧多场景的 motion 顺序**：motion 的描述符、keyframe 和时钟缓冲也由 painter 共享。同帧第二个及之后的 paint 只将变化的数据按 encoder 录制位置上传，避免后一个场景覆盖前一个场景的动画；首个 paint 继续使用帧上传批，相同 motion 数据不重复上传。按序拷贝记入上传字节，不增加提交或 CPU 等待；丢帧通过同一共享资源账本使 motion 的 CPU 上传缓存失效，下一帧重新上传。此规则同时覆盖 `paint` 和 `paint_target`。
- **painter 资源计数**：`SceneWgpuPainter::text_glyph_counters()` 报告字形图集、raster 缓存与字形上传；`resource_counters()`（`ScenePainterResourceCounters`）报告图标图集个数与字节、URL 纹理个数、已上传 GPU 字节（含全部 mip，不含驱动填充）、待上传 CPU 字节与进行中的加载。headless 宿主经 `PresentationDriver::text_glyph_counters()` / `painter_resource_counters()` 读取共享 painter 的同一组数字。均为 painter 分配的纹素字节，不是驱动显存。
- **目标附件与管线复用**：UI-only 与 GPU 节点混排之间切换时，只增加或释放 4x MSAA 附件，保留同尺寸、同格式及同 presentation profile 的线性颜色目标、绑定和组缓存。缩放必须重建尺寸相关纹理，但设备管线缓存命中时不再创建着色器模块或管线布局，避免多目标首帧重复解析 WGSL。改变尺寸、输出格式或 presentation profile 仍按原合同重建目标。


## 挂载宿主纹理（`HostTexture` + `GpuTextureView`）

着色器输出、预览视口、离屏渲染纹理与普通按钮一样，是界面树上占据布局、受裁剪、可命中的节点。首次接入请使用默认路径：把画面画到可采样纹理上，在树上挂载 `GpuTextureView`，并用同一个字符串槽位名在 `host_textures()` 中登记。

```bash
cargo run -p nana-ui --example hosted-gpu-demo --features hosted,bundled-fonts,wgpu-interop
```

`run_runtime` 会创建进程唯一的一份 `GpuContext`（附加窗口只新建 Surface 并共享该上下文；外部事件循环则通过 `EmbeddedRuntime` 与 `HostedGpuShared::from_device` 注入现有设备，切勿再次 `request_device`）。

宿主创建及恢复共享设备时，自动启用适配器支持的 BC／ASTC 纹理压缩能力，供内容生产端在同一设备上采样压缩纹理；这不会改变 UI 自身的纹理存储策略。不支持的能力不请求，生产端按设备实际能力选择格式或回退。外部注入设备和应用离屏宿主须在创建设备时自行请求所需的压缩能力。

:::api

```rust view
// 树上：和 Button 一样占布局
let preview = cx
    .mount_view_root(document_id, || view! {
        <Texture resource="preview" />
    })?
    .root::<GpuTextureView>();
```

```rust rust
// 树上：和 Button 一样占布局
let preview = cx
    .mount_view_root(document_id, || widget(GpuTextureView::new("preview")))?
    .root::<GpuTextureView>();
```

:::

在 `RuntimeProgram` 中登记并按帧更新该纹理槽：

```rust
// 1. initialize / rebuild_gpu：在宿主设备上创建纹理并登记槽位
let gpu = context.gpu();
let texture = gpu.create_texture(&GpuTextureDescriptor {
    label: Some("preview"),
    width,
    height,
    format: GpuTextureFormat::RGBA8_UNORM_SRGB,
    usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
})?;
self.textures.register(
    "preview",
    HostTexture::new(&texture),
    width,
    height,
    HostTextureAlphaMode::Opaque,
);

fn host_textures(&self, _id: WindowId) -> Option<HostTextureRegistry> {
    Some(self.textures.clone())
}

// 2. flush 之前更新像素或替换纹理
fn prepare_window_frame(&mut self, id: WindowId, context: &RuntimeProgramContext<Self::Message>) {
    // 上传 CPU 像素：context.gpu().write_texture(&texture, region, &pixels, bytes_per_row)
    // 替换纹理对象或尺寸：host.replace_texture(&new_texture)；仅内容更新：host.invalidate()
    // 注意：切勿为了换纹理而去修改 Runtime 节点；窗口遮挡或 0 尺寸时仍可能调到这里，不可假定随后必有 present
}

// 3. present 之后才可释放上一帧被替换下的旧纹理
fn window_frame_presented(&mut self, _id: WindowId, _context: &RuntimeProgramContext<Self::Message>) {}

// 4. 设备丢失恢复：程序实例保留，仅在新的 context.gpu() 上重建 GPU 资源
fn rebuild_gpu(&mut self, context: &RuntimeProgramContext<Self::Message>) {}
```

- **槽位是唯一身份**：视图只记槽名（`TextureSlotName`），每帧绘制时按槽名取注册表里当前的绑定。`HostTexture::new` 自己分配 id；往同一个槽登记一个新句柄、`replace_texture` 换纹理对象（`generation` 递增）或 `invalidate` 刷新内容（`version` 递增），宿主都会重绘用到这个槽的窗口，不用改 Runtime 节点。设备重建时宿主先清空注册表，再调 `rebuild_gpu`；程序在新设备上重新登记即可。若 `HostTexture::device_generation()` 与当前 painter 的设备代际不符，整帧会以 `ScenePaintError::StaleHostTexture` 拒绝。
- **线性工作空间要求**：进入 Scene 的 `HostTexture` 采样结果必须处于预乘线性 scRGB 空间——普通不透明/直通 alpha 的 sRGB 图片可使用 `*_SRGB` 纹理格式交由硬件采样器解码；带半透明的预乘内容请使用线性 `UNORM` 或浮点格式，勿将 gamma 空间预乘字节误标为 `SRGB`。

### GPU 合同与 wgpu 逃生口

#### 逻辑 GPU ABI（Issue #185）

普通渲染扩展可通过 `nana_gpu::ResourceTable` 声明 uniform / storage 缓冲、采样/存储纹理、采样器、动态切片与可选资源数组，并通过 `ShaderInterface` 绑定可移植 WGSL 与顶点/实例布局（`GpuContext::create_resource_layout` 与 `create_resource_group` 会在创建不透明句柄前校验 `DeviceGeneration`、usage、范围与可选能力，公开 API 不泄漏 `wgpu::BindGroupLayout`）：

| 核心合同类型 | 职责说明 |
| --- | --- |
| `GpuContext` | 进程唯一的 GPU 设备句柄：携带 `generation()`、`capabilities()`、粘性丢失状态 `is_lost()`，提供 `create_texture` / `write_texture` / `begin_frame` |
| `FrameContext` | 单帧独占编码器：`submit()` 提交并返回 `GpuSubmission`；drop / `discard()` 立即作废并自动回滚 painter 登记在该帧上的保留目标状态 |
| `GpuTexture` / `GpuRenderTarget` | 携带 `DeviceGeneration` 的 2D 纹理与渲染目标（跨代际资源在入口即被拒绝） |
| `GpuTextureFormat` / `GpuTextureUsages` | 后端中立的不透明格式与用途标志 |

当自定义渲染器确实需要访问底层原生 WGPU 对象时，显式启用 `wgpu-interop` feature（提供 `GpuContext::from_wgpu` / `wgpu()`、`FrameContext::wgpu_encoder()`、`GpuTexture::from_wgpu` / `wgpu_view()`、`ScenePass::wgpu()` 等，交出的是合同背后的**同一个**设备与编码器）。框架内部 crate（`nana-ui`、`nana-frame-exchange`、`nana-ui-devtools`）则通过隐藏的 `nana_gpu::__framework` 访问后端，由 `scripts/check-engine-boundary.py` 在 CI 中强制把关。

::: warning 提交守卫不可重入
窗口缩放执行 `Surface::configure` 时会等待 GPU 空闲；若其他线程同时提交队列会导致 `GpuWaitTimeout`。因此通过 `wgpu-interop` 直接调用裸 `queue.submit` / `write_texture` / `write_buffer` 时，必须持有 `gpu.wgpu().lock_submission()` 读锁，且**严禁跨越 `device.poll(Wait)`、休眠或 Surface 操作持有该锁**。`FrameContext::submit`、`GpuContext::write_texture` 与 `FrameExchange::copy_from` 内部已自动加锁，切勿在已持锁时嵌套调用它们。
:::

#### 原生纹理导出（`native-export`）

独立创建 WGPU 设备、且没有既有 device-lost callback 的外部生产端，使用 `GpuContext::from_wgpu_with_loss_tracking` 明确把 callback 交给同一 `GpuContext`；所有 clone 的 `is_lost()` / `lost_report()` 共享状态。原 `from_wgpu` 继续保留宿主 callback，不会自动替宿主观察丢失；已有宿主仍自行转发给 embedded runtime。OpenVR / Electron 参考生产端采用前者，避免声明检查 `is_lost()` 却从未安装通知。`controlled_device_loss` 集成测试实际调用 `device.destroy()`，验证 callback 所有权、粘性丢失与替换代际拒绝；这不代表硬件 driver reset 或外部 compositor 的恢复已实测。

在 Windows + DX12 设备上启用 `native-export` feature（对应能力位 `GpuCapability::NativeTextureExport`）后，`NativeExportPool` 可在宿主同一份设备上创建 3 张共享的 `BGRA8` 纹理与一把共享围栏并导出为 NT 句柄，供外部 D3D11 / Spout 消费方零 CPU 回读地跨 API 采样。完整围栏时序（`2k + 1` 就绪、`2k + 2` 释放）与消费方合同请见 [窗口外输出 · DX12 共享纹理](output.md#dx12-共享纹理native-export)。

## 跨线程最新帧交换（`FrameExchange` 与 `FrameBinding`）

当实时画面在独立线程上生产（如模型渲染、导播合成、视频解码）时，使用 `nana-frame-exchange` 的 `FrameExchange` 将完成的帧交给窗口，并在窗口侧用 `FrameBinding` 绑定到槽位（生产端仅需依赖 `nana-frame-exchange`）：

```rust
// 生产线程：每个节拍调用 poll，有新帧时录制并 copy_from
let mut exchange = FrameExchange::new(&gpu, frame_exchange::DEFAULT_CAPACITY, epoch, notify);
let inbox = exchange.inbox(); // 发送给 UI 线程

let mut frame = gpu.begin_frame("producer");
scene.render(frame.wgpu_encoder()); // wgpu-interop
frame.submit();
match exchange.copy_from(&rendered, epoch) {
    CopyOutcome::Submitted => {}
    CopyOutcome::PoolFull => {} // 池满直接丢弃本帧，不阻塞等待
    CopyOutcome::EmptySource | CopyOutcome::IncompatibleSource | CopyOutcome::DeviceMismatch => {}
}
exchange.poll();

// UI 窗口线程
let mut binding = FrameBinding::new(
    context.gpu(),
    textures.slot("program"),
    HostTextureAlphaMode::Premultiplied,
);
// 在 prepare_window_frame 中调用：binding.prepare(Some(&inbox), |token| accept(token));
// 在 window_frame_presented 中调用：binding.presented(Some(&inbox), |token| accept(token));
```

- **GPU 内部复制与无锁交换**：`copy_from` 在纹理池内执行一次 `copy_texture_to_texture`（要求源纹理具备 `COPY_SRC`、单采样 2D、非深度格式）。单扇消费窗口建议分配 `3` 个槽位（在途复制、正在显示、已替换待 present），每增加一扇共享消费窗口多加 `2` 个槽位。
- **租约顺序与 Epoch 过滤**：`prepare` 换上新帧后，旧帧一直保留到 `presented` 才归还，生产端需等采样它的那次 UI 提交完成后才会复用该槽位。应用可通过 `set_epoch` 推进场景代次（立即隐藏收件箱中的旧代次帧；在新代次帧尚未发布且 `accept` 仍允许旧帧时，`FrameBinding` 会保持已显示的画面而不闪回透明空白）。

## GpuView：同 Pass 无中间纹理直写

当画面没有中间纹理、必须直接编码进当前 UI 渲染通道时，使用 `GpuView::new(slot_id)`（投影渲染器键为 `"gpu-view"` 的 `CustomRenderNode`，演示程序见 `gpu-view-demo`）：

- **`Inline` vs `Standalone`**：`GpuViewMode::Inline` 直接复用 painter 当前打开的目标 pass（通过 `ScenePass` 设置 viewport 与 scissor）；`Standalone` 则通过 `SceneGpuRenderContext::with_pass` 在同一帧、同一目标上另开一个 `Load / Store` pass。自定义渲染器输出的颜色必须为**预乘线性 scRGB**，管线缓存键应设为 `(context.gpu.generation(), context.target_format, context.presentation)`。
- **保持文档顺序的实例化合批**：若实现 `batch_capacity() > 1` 与 `draw_batch_in_pass`，painter 会把显示列表上**连续紧邻**的同渲染器节点切片交给它一次绘制；中间一旦出现任何普通 Quad、文字、图标、HostTexture 或合成组边界，切片即告终止，从而保证遮挡顺序与逐节点绘制逐比特一致。多节点共用同一着色器时请使用相同的 `(slot_id, revision)` 并将每节点差异放入 `params`。
- **实现 `preparation_version` 以允许帧缓存**：自定义渲染器默认每帧重新 prepare；**只要树上存在一个未实现 `preparation_version` 的自定义渲染器，painter 就会将整帧判定为不可缓存并重建全部绘制命令**。请像内置的 `DefaultGpuViewRenderer` 一样按 `(revision, params)` 实现该方法。

## 媒体与 GPU 槽位合同

| 节点 / 接口 | 底层槽位合同 | 行为说明 |
| --- | --- | --- |
| `GpuTextureView` | `"nana.host-texture"` + 宿主登记的槽位名 | 标准宿主纹理节点 |
| `GpuView` | `"gpu-view"` + 十进制 `slot_id` | 宿主需注册对应 `SceneGpuRenderer` |
| `Video` | `"nana.host-texture"` + `video:{id}` | 宿主推帧（有槽位时不画 `poster`；无槽位仅显示静态 `poster` 图片，不解码音频/视频） |
| `BrowserView` | Runtime 布局锚点 + 宿主原生子视图 | 仅 macOS `WKWebView`（Windows/Linux 返回 `Unsupported`），见下节 |

### 按实际绘制像素准备内容

`HostTextureRegistry::painted_extent(slot)` 返回该槽位在各渲染目标最近一次完整绘制中所需的最大设备像素尺寸（已计入 `ContentFit`、节点自身变换与当前 DPI 缩放系数；当同一槽位被列表缩略图与大图浮层同时引用时，自动取最大消费者尺寸）。生产端可通过 `painted_revision()`、`painted_changes_since(rev, &mut out)` 或 `subscribe_painted(callback)` 监听尺寸需求变化并按真实物理分辨率准备纹理，切勿自行用「上一帧的 `LayoutBox` × 缩放因子」手算代替。

## 应用内浏览器

`BrowserView` 是窗口宿主的原生子视图例外，而非整窗 WebView 壳：Runtime 节点负责布局、无障碍与可见性锚点，宿主管理原生引擎子视图（当前仅支持 macOS `WKWebView`，不进入离屏 Scene 截图）。

### 无头网页画面（WebSurface）

`WebSurface` 将平台网页引擎实例完全运行在窗口视图树之外，按 `max_fps` 截取页面帧（`WebFrame`：紧凑排列的 RGBA8、sRGB 空间预乘 alpha）并在后台线程送入应用提供的 `WebFrameSink`，UI 线程零像素转换、零排队开销。应用可自由决定帧的去向（如写入 `HostTexture` 槽位或自定义合成器）：

- **请求对账与原地更新**：程序在 `RuntimeProgram::web_surface_requests()` 中声明 `WebSurfaceRequest { id, policy, desc, restore_url, revision, command, frames }`（撤回请求即释放引擎），状态事件通过 `web_surface_event` 回流。修改 `desc`（尺寸、缩放、透明、帧率）原地生效；更换 sink 或安全策略则会重建实例。
- **临时弹出交互窗口（`ShowWindow`）**：可将同一个无头页面临时放进可交互的原生窗口（用于完成网页登录或点击页面内设置），期间 Cookie、页面状态不丢失且画面帧持续输出；用户关窗或发送 `HideWindow` 后页面回到屏外并上报 `WindowClosed`。
- **平台后端**：macOS 基于屏外无边框 `WKWebView`（自动检测连续 10 帧静止画面并降频至 4 fps，画面一动立即恢复；可通过 `cargo run -p nana-window --example web-surface-probe` 实测）；Windows 基于独立 STA 线程上的 WebView2 Composition Controller + `Windows.Graphics.Capture`（需系统安装 WebView2 Runtime 并随包分发 `WebView2Loader.dll`）；Linux `web_surface_support()` 返回 `false`。

## 按图离屏

当离屏纹理必须根据当前 Scene 图在采样**之前**动态编码生成时，树上仍挂载 `GpuTextureView`，并在宿主侧实现 `SceneResourceProducer`：

- **可见帧合批提交**：`encode_scene(scene, &mut frame)` 直接录制进宿主获取 Surface 后开启的同一个 `FrameContext`，与 UI 绘制在同一次 `queue.submit` 中提交（编码或绘制任一失败则整帧丢弃，生产者严禁自行提交编码器或提前谎报完成）。
- **隐藏窗口节拍（Hidden Tick）**：当窗口被遮挡、最小化时，`FrameDemand` 到期仍会调用 `prepare_window_frame` 并在无 Surface 的独立 `FrameContext` 上执行 `scene_resource_producers` 后立即提交（尺寸为 0 时仅 prepare 而不执行 producer 编码）。
- **单帧单目标铁律**：`FrameContext` 独占管理帧的提交与丢弃回滚。若前一帧绘制过某个 `RenderTargetId` 但尚未提交也未丢弃，再次绘制该目标会返回 `ScenePaintError::TargetInFlight`。设备丢失记在粘性的 `GpuContext::is_lost()` 上；外部通过 `EmbeddedRuntime` 注入设备时，宿主应在收到设备丢失通知后调用 `EmbeddedRuntime::notify_device_lost()`，并在新设备就绪后调用 `replace_gpu()`。

### GPU 节点的准备缓存与画面修订

`SceneGpuRenderer::preparation_version` 只控制 UI 命令准备的复用。按时间或外部状态绘制的节点即使资源和几何不变，像素也可能每帧变化；这类渲染器应让 `content_version` 返回 `None`。窗口保留输出、窗口外 PresentationDriver 与 Spout 共享输出据此按宿主请求重新绘制，同时仍复用稳定的准备命令。

静态或显式修订的渲染器可返回 `Some(revision)`，并在像素发生变化时递增。为保持已有消费者行为，`content_version` 默认沿用 `preparation_version`；动态渲染器需要显式区分二者。安装了动态渲染器但当前场景未引用它，不会使静态输出持续绘制。

通用文字与形状片元材质见 [通用材质](materials.md)。

### 逐帧应用载荷

`FrameExchange<E, M = ()>` 的 `M` 是随 GPU 拷贝一起发布的不可变应用载荷，与 epoch 和 `FrameToken<E>` 身份分离。通过 `new_with_payload` 构造，`copy_from_with_payload`（或 `wgpu-interop` 下的 `copy_from_wgpu_with_payload`）同时提交纹理、颜色元数据和载荷；`FrameLease::payload()` 与 `FrameBinding<E, M>::payload()` 只返回对应 lease / 当前绑定帧的数据。原有无载荷构造与复制接口保持兼容。

载荷在 pending copy、完成发布、绑定和呈现后退役之间与纹理共同保留；丢帧、代际失效和槽回收遵循原规则。更新载荷不切换 epoch、不清空最新帧，也不新增历史索引或扩大纹理池。消费者不得将旧纹理配上另读的最新生产状态。载荷类型由应用定义，框架不依赖应用的坐标或模型类型。
