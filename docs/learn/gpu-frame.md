---
title: GPU 与帧生命周期
---

# GPU 与帧生命周期:nana-gpu 合同与宿主的一帧

这一章回答三个问题:**`nana-gpu` 的类型合同(GpuContext/FrameContext/GpuTexture、代际与丢失、提交守卫)为什么长这样;`nana-ui` 的 `SceneWgpuPainter` 如何在宿主上下文上组织 pass;一帧从调度到 present 的完整路径经过哪些手**。读完你能搞懂:谁拥有 Device/Queue/Surface/编码器;帧槽与代际怎么防止竞态;`CustomRenderNode` 与 `HostTexture` 两条 GPU 内容挂树路径的区别;`nana-frame-exchange` 运什么、不拥有什么;以及 `wgpu-interop` 逃生舱何时允许打开。它承接 [Scene 提取](/learn/scene-extraction)的 `UiScene`,向下接 [窗口与平台](/learn/window-platform)。

## 职责与边界

- **`nana-gpu` 是合同不是后端**:crate 文档列出全部承诺——`GpuContext` 单设备(代际、能力、丢失状态、纹理创建/上传、提交守卫);`FrameContext` 从 `begin_frame` 到 submitted 或 dropped 独占一个编码器(drop 即 discard 并回滚保留写);`GpuTexture`/`GpuRenderTarget` 携带设备代际,换设备后的资源被拒绝(`crates/nana-gpu/src/lib.rs:1`)。"WGPU is the only backend... `wgpu-interop` feature is the explicit escape hatch"。
- **所有权表(AGENTS.md 合同)**:宿主拥有 Window、Surface、`GpuContext`、每个 `FrameContext` 与帧调度;`SceneWgpuPainter` 建立在该上下文上,画进宿主的帧。全仓**一个** wgpu 大版本(30.0.0),一个 Device/Queue,没有产品 CPU 回读路径。
- **`nana-frame-exchange` 只运帧**:模块文档自述 "Bounded latest-frame exchange on a host's existing GPU... It does not own the UI device or submission"(AGENTS.md);拷贝是 GPU 侧 `copy_texture_to_texture`,不是零拷贝,也永不回读 CPU(`crates/nana-frame-exchange/src/lib.rs:1`)。
- **边界之外**:GPU 内容不是窗口外的浮层——它以 `CustomRenderNode` 或普通 `HostTexture` 节点表示,仍在布局、裁剪、命中测试与文档序里(见[架构:GPU](/architecture/gpu))。

## 依赖与邻居

层级:core(L1)→ **gpu(L2)**→ frame-exchange(L3)→ runtime(L5)→ scene(L6)→ **nana-ui(L7)**。`nana-ui` 是唯一持有 `SceneWgpuPainter` 的 crate;`nana-frame-exchange` 依赖 `nana-gpu`(经 `nana_gpu::__framework`),并再导出 `GpuContext`/`GpuTexture`/`DeviceGeneration`/`GpuTextureFormat` 作为合同类型——"so a producer crate needs no other GPU dependency"(`crates/nana-frame-exchange/src/lib.rs` 模块文档)。

| 邻居 | 关系 |
| --- | --- |
| `nana-ui-scene` | painter 的输入:`UiScene` 原语、`frame_plan()`、`instance` 代际 |
| `nana-window` / `nana-ui-platform` | Surface 与呈现路径(透明/合成)归窗口层,见 [窗口与平台](/learn/window-platform) |
| `wgpu` | 唯一后端;公共 GPU 扩展合同只用 `nana-gpu` 类型,`wgpu::*` 只能经 `wgpu-interop` 出现在 nana-gpu / nana-frame-exchange / nana-ui / nana-ui-vue / nana-ui-devtools 的公共签名(`scripts/check-engine-boundary.py` 强制) |
| `nana-diagnostics` | 帧路径零格式化/零 I/O;`FRAMES_DISCARDED`、`FRAME_SLOTS_EXHAUSTED` 等是静态描述符事件或指标 |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `GpuContext` | `crates/nana-gpu/src/context.rs:358` | 设备句柄(`Arc<GpuInner>`,克隆廉价、同代际);能力、丢失、纹理、begin_frame |
| `DeviceGeneration` | `crates/nana-gpu/src/context.rs:19` | `NonZeroU64` 设备代际,进程级计数器分配 |
| `GpuCapabilities` | `crates/nana-gpu/src/context.rs:204` | 从 adapter/device 读出的特性表(`read`) |
| `FrameContext` | `crates/nana-gpu/src/frame.rs:38` | 一帧:编码器 + 保留写账本 + 帧槽 + 帧上传批 |
| `GpuSubmission` | `crates/nana-gpu/src/frame.rs:301` | 提交回执:帧、代际、cpu_duration、提交序号 |
| `RetainedWrites` | `crates/nana-gpu/src/frame.rs:360` | 跨帧保留写账本:丢弃回滚、in_flight 防双录 |
| `GpuTexture` / `GpuRenderTarget` | `crates/nana-gpu/src/texture.rs:183` / `:276` | 带代际的 2D 纹理 / 可渲染目标(表面视图) |
| `GpuDeviceState` | `crates/nana-gpu/src/policy.rs:232` | 每设备策略权威:上传环、帧槽、管线注册表、退休回收 |
| `SceneWgpuPainter` | `crates/nana-ui/src/scene_paint/mod.rs:381` | 内建 WGPU 画家:管线家族 + 每目标状态 |
| `RenderTargetId` | `crates/nana-ui/src/scene_paint/mod.rs:463` | 宿主拥有的窗口/离屏呈现目标身份 |
| `DrawCommand` | `crates/nana-ui/src/scene_paint/mod.rs:513` | 录制后的绘制命令枚举(Quads/Mesh/Text/Icon/HostTexture/Custom/…) |
| `HostTexture` / `HostTextureRegistry` | `crates/nana-ui/src/gpu_texture.rs:370` / `:555` | 宿主纹理句柄(版本+代际)与共享槽位注册表 |
| `SceneGpuRenderer` | `crates/nana-ui/src/scene_gpu.rs:247` | 高级 in-pass 编码合同(custom 节点) |
| `SceneResourceProducer` | `crates/nana-ui/src/scene_gpu.rs:325` | 图调度的离屏准备(HosTexture 路径) |
| `FrameExchange` / `FrameBinding` | `crates/nana-frame-exchange/src/lib.rs` / `crates/nana-ui/src/frame_binding.rs:25` | 生产者帧交换 / 消费侧绑定 |

## 内部结构

### nana-gpu 合同:代际、丢失与提交守卫

**代际**。`DeviceGeneration(NonZeroU64)`(`crates/nana-gpu/src/context.rs:19`)由进程级 `NEXT_DEVICE_GENERATION` 分配;每个 `GpuTexture`/`GpuRenderTarget` 记住创建它的代际,`GpuContext::check_device`(`context.rs:715`)在跨代使用时返回 `GpuError::DeviceMismatch`——设备被替换后,旧资源不是悄悄变无效,而是被点名拒绝。**丢失**:`GpuLossReason{Destroyed, Unknown}`(`context.rs:298`),`LossState` 粘性保存**第一次**报告(`context.rs:312`–325),避免回调风暴;宿主经 `EmbeddedRuntime::notify_device_lost` 得到通知并走恢复(`recover_device`,见下文一帧生命周期第 2 步)。

**提交守卫**是 `nana-gpu` 最关键的一条内部不变量:`GpuInner.submission: RwLock<()>` 的文档写明它 "Serializes queue work against `Surface::configure`, which waits for GPU idle: a submit racing it from another thread fails with `GpuWaitTimeout`. Every submit and upload the contract performs holds it for reading only for the duration of the call; reconfiguration takes it for writing"(`crates/nana-gpu/src/context.rs:327` 附近)。读写两端由 `lock_submission()`(`context.rs:726`,读)与 `lock_reconfigure()`(`context.rs:733`,写)暴露;`wgpu-interop` 的使用者必须持同一把读锁跨所有裸 `queue.submit`/`write_texture`/`write_buffer`,且"Never hold it across `device.poll(Wait)`, a sleep, or surface work... It is not reentrant"(`crates/nana-gpu/src/wgpu_interop.rs:45`)。

**验证先于后端**:`create_texture`(`context.rs:459`)在到达后端前校验 extent/usage/块对齐/格式用途,"never a validation panic";`GpuError` 因此全是合同拒绝(`DeviceMismatch`、`MissingUsage`、`RowTooShort`…,`crates/nana-gpu/src/error.rs`,#[non_exhaustive])。

### FrameContext:一帧的编码器、提交与丢弃

`FrameContext`(`crates/nana-gpu/src/frame.rs:38`)拥有一个编码器,两条出路:

- **`submit(mut self) -> GpuSubmission`**(`frame.rs:148`):先在守卫**外**做 `encoder.finish()` 与上传批打包(分块可能等待,不能占着锁);然后持读锁 + `submission_order` 互斥量,把上传命令缓冲链在帧命令**之前**一次 `queue.submit`;随后结算保留写、标记帧槽已提交、`on_submitted_work_done` 回调里释放帧槽与瞬态缓冲并回收退休资源。没拿到帧槽的帧也保住池化缓冲的完成保证(`frame.rs:219`)。
- **`discard(self)` 或 Drop**(`frame.rs:250`/`:259`):编码器先落下、计 `FRAMES_DISCARDED`;帧内上传并入设备待发批("Queue writes cannot be taken back... the next submission lands them"——排队写收不回,下一提交落地);保留写走 `RetainedWrites::roll_back`,每账本发一次 `RETAINED_FRAME_DISCARDED` 事件(`frame.rs:439`)。

`RetainedWrites`(`frame.rs:360`)是「画进被丢弃帧的目标状态」账本:`has_rolled_back()` 原子可查、`drain_rolled_back(rebuild)` 每键恰好一次、`in_flight(key, frame)` 阻止跨帧双录。painter 用它保证:上一帧丢弃了,这一帧先重建目标状态再画。**帧槽**在 `policy.rs`:默认 3 个(`frame_slots: vec![None, None, None]`,`crates/nana-gpu/src/policy.rs:253`),`take_frame_slot`(`policy.rs:295`)满时返回最旧已提交槽的 `SubmissionIndex` 或 `Unsubmitted`;`begin_frame(label)`(`context.rs:624`)在满槽时对最旧已提交做一次至多 1 秒的 `poll(Wait)`,全未提交则**无槽开帧**并记 stall——而不是死等;`try_begin_frame`(`context.rs:652`)是宿主用的非阻塞版。`GpuDeviceState` 同时管瞬态预算(默认 64MiB,`policy.rs:242`)、`PipelineKey` 管线缓存(`StampedCache` 摊还 O(1) 淘汰,`policy.rs:19`)与编译去重(每键 `OnceLock`,第二个请求等待,`policy.rs:221`)。

**上传环形**(`crates/nana-gpu/src/upload.rs:1`)解决的是"每个小写一次 `queue.write_buffer`,Metal 上一次就是上万条指令":写先进 `FrameContext` 的 CPU 批,提交时两批 memcpy 进 `UploadRing` 的一个映射分块,一条上传命令缓冲每目的段记一次拷贝,与帧命令**同一次** `queue.submit`、排在前面。环是整块池(mmap 的缓冲不能被提交占用,分块经 map 回调归还);`ROW_ALIGNMENT`/`TEXTURE_OFFSET_ALIGNMENT = 512`(D3D12 放置对齐)/`MIN_CHUNK = 1MiB`/`RING_BUDGET = 32MiB`/`RING_WAIT = 250ms`(`upload.rs:27`–37)。

### 宿主适配:SceneWgpuPainter 与 pass 组织

`SceneWgpuPainter`(`crates/nana-ui/src/scene_paint/mod.rs:381`)是"画 `UiScene` 的内建 WGPU 画家"。模块文档先立边界:宿主拥有 `GpuContext` 与每帧的 `FrameContext`;"`HostTexture` is sampled in document order inside the current dest (or opacity-group) pass; groups do not open a pass per HostTexture slot"——host 纹理在当前 dest(或不透明度组)pass 内按文档序采样,组**不**为每个槽开新 pass。

结构上是「设备级管线 + 每目标状态」两层:

- **设备级**(构造一次):quad/motion/mesh/icon/text/host_texture/backdrop 七个管线家族,`charts: Option<ChartPipeline>` 首图才建;构造时经 `beside!` 宏在 scoped 线程上**并行编译**(quad 最慢留在本线程,`mod.rs:566`);`dest_pipeline_cache` 仅当宿主开启了 `PIPELINE_CACHE` 能力才建(`mod.rs:678`–687)。`nana-ui` 自己的源码经 `nana_gpu::__framework` 摸 WGPU,不需要开 `wgpu-interop`。
- **每目标**(`TargetState`,`mod.rs:469`):prepared batch、各管线实例、dest、image_revision 等,按 `RenderTargetId(pub u64)`(`mod.rs:463`,"Host-owned identity for a window or offscreen presentation target")挂表,`swap_target_state`(`mod.rs:935`)换入换出。**保留写两条账**:`retained_targets`(记进被丢弃帧的目标,下次 paint 前重建)与 `retained_default`(画家自持状态,键 `DEFAULT_TARGET = 0`,`mod.rs:466`)。

`paint_target(id, scene, frame, target, viewport, host_textures, gpu_renderers)`(`mod.rs:899`)的骨架:`begin_frame`(`mod.rs:955`)先清 GPU 工作记录、校验 `frame.generation() + target.generation()` 对画家代际(不符 → `ScenePaintError::DeviceMismatch`)、查 `in_flight(target)`(同目标两帧在飞可能乱序提交 → `TargetInFlight`)、查目标账本是否回滚过(回滚过 → 排空并丢弃该目标状态;对照 `remove_target`(`mod.rs:871`)整目标关闭但保留取回宿主与图像缓存);然后换入目标状态跑 `paint_recorded`(`mod.rs:1049`):`validate_scene` **先于一切**、零物理尺寸早退、`url_cache.poll()` 推 `image_revision` 使图像绑定失效、dest 复用快路径(内容没变且无 GPU 动画在飞 → 只重设呈现参数加一次 blit)。成功后 `frame.record_retained_writes(&retained_targets, id.0)`。颜色合同:`ScenePresentationProfile`(`mod.rs:94`)——"Scene primitives are always composed as premultiplied linear scRGB; this describes only the output boundary";需要浮点工作区时 `working_format()` 取 RGBA16_FLOAT(`mod.rs:177`);`AlphaEncoding`(`mod.rs:444`)区分离屏/不透明目标的 Linear 与被合成器混的表面的 Gamma。录制产物是 `PreparedBatch`(`mod.rs:486`)里的 `Vec<DrawCommand>`(`mod.rs:513`):Quads/Mesh/Path/Chart/PathMask/Text/Icon/HostTexture/Custom/Backdrop/PushGroup/PopGroup,scissor 一律物理像素矩形。

### 一帧的完整生命周期

宿主实现在 `crates/nana-ui/src/scene_host/present.rs:114` 的 `redraw`,完整序列(编号即代码顺序):

| # | 步骤 | 要点 |
| --- | --- | --- |
| 1–2 | 指标回收;设备丢失检查 | `take_device_lost` → `recover_device`(换新 `GpuContext`,旧代际资源被拒) |
| 3–4 | 排干程序消息并应用;窗口 resize | 输入 dispatch / update 在这里进树 |
| 5 | `program.prepare_window_frame(id, ctx)` | 程序的帧前钩子(见「协作与数据流」) |
| 6–11 | 输出同步、合成钟、文档 flush、光标、a11y stage、共享 scene | `document.flush(viewport, &mut text)` → `RuntimeFrameUpdate`;`FrameDidNotSettle` 则报告并跳过,runtime 的脏工作已还原 |
| 12–13 | 表面 profile 与呈现参数;启动接管 | HDR headroom 是 uniform,不是管线键 |
| 14 | **`reserve_scene_host_frame(gpu)`**(`present.rs:12`) | `try_begin_frame` 失败先 `poll(Poll)` 一次再试;仍无槽就放弃本帧(计 `FRAMES_SKIPPED`)。**在拿 drawable 之前**——注释:"a transparent swapchain has nothing else to composite once its drawable is taken... with every slot in flight the presented frame stays on screen and this one is skipped" |
| 15–16 | `acquire_frame` → 表面纹理 → `GpuRenderTarget` | Ready/Retry/Skipped/Err 四态,Err 走表面挂起 |
| 17 | `producers.encode_scene(scene, &mut recording)` | `SceneResourceProducer` 的准备 pass 录进同一 `FrameContext`;失败则 discard + `HostFailure::ResourceProduction` |
| 18–19 | host-texture 槽订阅;Windows 原生内容 | 从 `scene.frame_plan()` 的 custom_nodes 过滤 `renderer == "nana.host-texture"` 得槽集合,订阅重绘唤醒;原生内容渲染器按 working_format 注入 |
| 20–21 | 画家参数;**`painter.paint_target(...)`**(`present.rs:385`) | 失败 → discard + `UnpaintableScene` + 重整 |
| 22–24 | 窗口输出二次绘制;**`recording.submit()`**;完成回调 | 窗口输出把同一 scene 画进自己的目标,同一帧、同一提交 |
| 25–27 | `present_frame`;帧呈现诊断;应用挂起的重配置 | 重配置在 present 之后,配合提交守卫的写锁 |
| 28–31 | 帧需求服务;图像尺寸提交;Windows 合成提交;窗口输出投递 | `commit_image_sizes` 注释:"Before any program callback can move the document away from the scene just painted";合成树"the host publishes, and only when something was staged" |
| 32–35 | a11y 同步;IME 刷新;`window_frame_presented` | `refresh_window_input` 的注释:"Layout moved the caret..." |

隐藏窗口走 `tick_hidden_gpu`(`present.rs:22`):仍跑 `prepare_window_frame` 与输出同步,但不 flush、不拿 Surface、不 present。

### CustomRenderNode 与 HostTexture:GPU 内容如何挂树

两条路径,都在树上、都在布局/裁剪/命中/文档序里:

- **`HostTexture`(普通路径,产品默认)**:`CustomRenderNode{renderer, resource, revision, fit, params, dedicated_pass, checkerboard, zoom, sampling}`(`crates/nana-ui-runtime/src/components.rs:4057`)由 `GpuTextureView` 投影成槽位字符串,renderer 固定 `"nana.host-texture"`;宿主在 `RuntimeProgram::host_textures(id)`(`crates/nana-ui/src/runtime_host.rs:665`)交出 `HostTextureRegistry`。`HostTexture`(`crates/nana-ui/src/gpu_texture.rs:370`)是稳定句柄:"`generation` changes only when the texture is replaced, while `version` changes for every content invalidation"(`gpu_texture.rs:362`);`replace_texture`(`gpu_texture.rs:496`)换纹理同时失效内容且**不重建布局**;`device_generation()`(`gpu_texture.rs:471`)让别设备画家拒绝采样而非崩溃。注册表只存视图:"It stores texture views only; Device/Queue ownership remains with the hosted renderer"(`gpu_texture.rs:552`)。
- **`SceneGpuRenderer`(高级 in-pass 路径)**:trait 文档直言 "Prefer `crate::HostTexture` / `crate::GpuTextureView` for first-time hosts";实现收到 `GpuContext` 与当前帧目标,缓存必须键在 `GpuContext::generation` 上,"must not create a second GPU context or submit the frame themselves"(`crates/nana-ui/src/scene_gpu.rs:247`)。`draw_in_pass` 默认 false——画家收掉主 pass 再调 `render`;能吃 dest 采样数与格式的实现选择加入。`draw_batch_in_pass`(`scene_gpu.rs:295`)的文档值得整段读:批次**不是**帧尾批,"The run is a slice of the display list: any quad, glyph, icon, host texture, backdrop or group boundary between two nodes ends it, so a node can never be drawn across ordinary UI. Document order is exactly what it would be without batching"。
- **`SceneResourceProducer`**(`scene_gpu.rs:325`):图调度离屏,对应 scene 侧 `FramePlan` 的 preparations;可见帧录在 Surface 编码器里随 UI 一起提交,隐藏 tick 无 Surface 立即提交;`submitted` 只表示已入队,"not that a UI frame sampling it has presented"。

### nana-frame-exchange 与 FrameBinding

`nana-frame-exchange` 把「生产者线程 ↔ UI 消费窗口」解耦:生产者把完成的帧拷进共享 `GpuContext` 上的小纹理池,消费者采样最新完成的拷贝;"Neither side waits for the other or for GPU completion: a full pool drops producer work, and a lease frees its slot only after the consumer submission that sampled it has completed"(`crates/nana-frame-exchange/src/lib.rs:1`)。帧的颜色描述 `FrameColorMetadata`(`lib.rs:108`)是**声明**不是转换:"copies texels byte-for-byte and does not perform a color conversion",`copy_from` 附 `unknown()` 元数据,"never silently labels an unknown source as sRGB"。消费侧 `FrameBinding<E>`(`crates/nana-ui/src/frame_binding.rs:25`)把槽位接进文档:"Call `Self::prepare` from `prepare_window_frame` and `Self::presented` from `window_frame_presented` of the window that samples the slot"(`frame_binding.rs:25`–33);槽位永远有绑定——没有可用帧时显示 1×1 透明占位("WGPU zero-initializes it: transparent black",`frame_binding.rs:51`–61),所以 scene 校验永不丢槽;epoch 已动但下一帧未发布时保持已显示画面。

### wgpu-interop 逃生舱

两条路摸原生 WGPU,动机截然不同:

- **`wgpu-interop`(公共签名)**:`WgpuInterop{gpu}`(`crates/nana-gpu/src/wgpu_interop.rs:18`)提供 `adapter()/device()/queue()`;`GpuContext::from_wgpu(adapter, device, queue)`(`wgpu_interop.rs:54`)把外部设备收编(丢失回调归调用者);`FrameContext::wgpu_encoder()` 的文档只有一句纪律:"Record into it; never finish or submit it"(`wgpu_interop.rs:129`)。允许出现在公共签名里的 crate 白名单由 AGENTS.md 固定,`scripts/check-engine-boundary.py` 强制。
- **`__framework`(框架内部)**:`crates/nana-gpu/src/__framework.rs` 的模块注释解释了为什么它必须存在:"This module exists because Cargo features unify — if the framework reached WGPU through wgpu-interop, every consumer would silently get the escape hatch too." `nana-ui`/`nana-frame-exchange` 自己的源码经它取原始对象,因此永远不需要为消费者打开 `wgpu-interop`。

## 协作与数据流

宿主与程序的合同是 `RuntimeProgram`(`crates/nana-ui/src/runtime_host.rs:473`)。与帧/GPU 相关的钩子及其文档语义:

- `gpu_backend_policy()`(`runtime_host.rs:487`):进程级后端要求,`CompositionCapable` 只是让合成路径**可用**,不把任何窗口放上去;
- `host_textures(id)`(`runtime_host.rs:665`):"Product default: attach an existing sampleable texture to the tree",配 `GpuTextureView` 同槽,在 `prepare_window_frame` 里更新;
- `scene_gpu_renderers(id)`(`runtime_host.rs:671`)/`scene_resource_producers(id)`(`runtime_host.rs:678`):高级注册;后者 "Same Device/Queue and host encoder; preparation precedes Scene sampling within the target's single submission";
- `prepare_window_frame(id, ctx)`(`runtime_host.rs:704`):"Acquire application-owned frame resources immediately before the host flushes and paints this window"——遮挡/最小化/零尺寸时仍被调用(隐藏 tick 不 flush、不拿 Surface、不调 `window_frame_presented`),"do not retire textures the last presented UI frame still samples";
- `window_frame_presented(id, ctx)`(`runtime_host.rs:714`):"Release resources retired by `prepare_window_frame` only after the host has submitted and presented";
- `window_output(id)`(`runtime_host.rs:732`):二次离屏输出(Spout、录制),"The host paints the window's own scene into the output inside the frame it records for the window, so nothing is laid out twice"。

数据流一图流:`UiWorld` --extract--> `UiScene`(见 [Scene 提取](/learn/scene-extraction))--frame_plan/order--> `SceneWgpuPainter`(校验代际、换目标状态、录 `DrawCommand`)--> `FrameContext`(上传环形、保留写账本)--> `GpuSubmission` --> present;`GpuContext` 的帧槽与提交守卫贯穿全程。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-gpu/src/lib.rs` | 合同总述与模块清单 |
| `crates/nana-gpu/src/context.rs` | `GpuContext`/代际/能力/丢失/提交守卫/begin_frame |
| `crates/nana-gpu/src/frame.rs` | `FrameContext` submit/discard/保留写账本/`GpuSubmission` |
| `crates/nana-gpu/src/texture.rs` | `GpuTexture`/`GpuRenderTarget`/稳定 usage 位 |
| `crates/nana-gpu/src/policy.rs` | 帧槽、瞬态预算、管线缓存、退休回收 |
| `crates/nana-gpu/src/upload.rs` | 上传环形(批合并、同提交前置) |
| `crates/nana-gpu/src/wgpu_interop.rs` | 逃生舱:`from_wgpu`/`wgpu_encoder`/锁纪律 |
| `crates/nana-gpu/src/__framework.rs` | 框架内部原生访问与 feature 统一问题 |
| `crates/nana-gpu/src/abi.rs` | 后端中立逻辑绑定 ABI(`ResourceClass`/`LogicalBinding`) |
| `crates/nana-ui/src/scene_paint/mod.rs` | `SceneWgpuPainter` 主体:目标状态、校验、录制 |
| `crates/nana-ui/src/scene_paint/` 子模块 | quad/text/mesh/icon/host_texture/backdrop/dest 等管线族 |
| `crates/nana-ui/src/scene_host/present.rs` | 宿主 `redraw`:一帧全序列 |
| `crates/nana-ui/src/gpu_texture.rs` | `HostTexture`/注册表/版本与代际 |
| `crates/nana-ui/src/scene_gpu.rs` | `SceneGpuRenderer`/`SceneResourceProducer` 合同 |
| `crates/nana-ui/src/frame_binding.rs` | `FrameBinding` 消费侧 |
| `crates/nana-frame-exchange/src/lib.rs` | 有界最新帧交换与颜色元数据 |
| `crates/nana-ui/src/runtime_host.rs` | `RuntimeProgram` 帧钩子文档 |

## 延伸阅读

- [Scene 提取](/learn/scene-extraction):`UiScene` 与 `FramePlan` 怎么来的;
- [窗口与平台](/learn/window-platform):Surface 生命周期、透明与合成路径;
- [架构:GPU](/architecture/gpu)与[架构:帧](/architecture/frame):应用作者视角的槽位表与 8 步帧;
- [工作原理](/reference/how-it-works)与[代码阅读路线](/learn/reading-the-code)。
