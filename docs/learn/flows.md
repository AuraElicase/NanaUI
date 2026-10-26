---
title: 端到端数据流
---

# 端到端数据流

前面各章按子系统纵向拆解了内部实现。本章反过来，挑选四条横跨多层的关键数据流，从输入源头一直追踪到屏幕像素或应用回调。每一步均标出所在 crate、核心类型与源码行号锚点——这是检验是否真正贯通整套框架的最佳方式。

## 流程一：一次状态变更如何变成屏幕像素

场景：应用代码修改了保留树，直到窗口 Surface 上呈现出新像素。

```text
① 变更入队    nana-ui-runtime   UiMutation（Create / Insert / Detach / SetStyle / …）
                                 crates/nana-ui-runtime/src/mutation.rs:15
② 帧调度      nana-ui           FrameDemand（OnDemand / 截止时间 / Continuous）
                                 crates/nana-ui/src/runtime_host.rs:1035
                                 宿主按需求决定本帧是否重绘；无变更时 flush 空转返回
③ 消息消化    nana-ui           run_runtime_scene 将 dispatch_program 的消息
                                 交给 RuntimeProgram::update（开窗 / 换 GPU / 持久化）
                                 crates/nana-ui/src/scene_host.rs:99
④ 准备纹理    nana-ui           prepare_window_frame：应用准备好本帧要采样的最新纹理
⑤ flush 事务  nana-ui-scene     RuntimeDocument::flush：样式 → 文本整形 → 布局 →
                                 命中索引 → 无障碍投影 → 脏节点抽取，单次事务收敛
                                 crates/nana-ui-scene/src/runtime_document.rs:109
⑥ 获取表面    nana-ui           获取窗口 Surface；外部资源生产者编入 FramePlan，
                                 与 UI 共用同一宿主 encoder（任一失败则整帧丢弃并回滚）
⑦ 录制绘制    nana-ui           SceneWgpuPainter::paint_target：可见操作按文档顺序录制进目标
                                 crates/nana-ui/src/scene_paint/mod.rs:899
⑧ 提交上屏    nana-gpu          FrameContext::submit（合并 UploadRing 上传批）+ present
                                 crates/nana-gpu/src/frame.rs:148
⑨ 帧后收尾    nana-ui           window_frame_presented：此时才可安全释放上一帧退役的旧纹理；
                                 bind_window 按需为新就绪窗口挂载后续内容
```

关键要点：

- `RuntimeDocument` 定义在 **`nana-ui-scene`**（`crates/nana-ui-scene/src/runtime_document.rs:18`），它同时持有 `AppContext`（保留树）与 `UiScene`（保留绘制图）——`flush` 就是将前者的脏变更推进到后者。
- 合成器动画与实时 GPU 画面的唤醒同普通 UI 隔离：`UiScene::compositor_needs_tick` 仅将当前窗口切入轻量的 `FrameDemand::Continuous` 呈现，不触发全局样式或布局重算。
- 被遮挡或最小化的窗口：宿主仍按节拍调用 `prepare_window_frame`，但跳过 `flush`、不获取 Surface、不 `present`。

## 流程二：一次点击如何变成应用回调

场景：用户在窗口中按下按钮，从操作系统原生事件一直到达应用注册的回调闭包。

```text
① 原生事件    nana-window / winit 操作系统窗口事件（平台相关结构）
② 边界降维    nana-ui-input       宿主在边界处将其降维为 CanonicalInputEvent
                                   （InputPayload / KeyInput / PointerInput，
                                   crates/nana-ui-input/src/canonical.rs:44）
③ 序列盖章    nana-ui-input       InputSequencer 盖上严格递增序列号与动画域时间戳
                                   （crates/nana-ui-input/src/canonical.rs:333）
④ 宿主服务    nana-ui-input       HostServices：光标、IME 与剪贴板能力经此合同回调
                                   （crates/nana-ui-input/src/host_services.rs:97）
⑤ 唯一路由    nana-ui-runtime     AppContext::route_input —— 全仓唯一路由实现
                                   crates/nana-ui-runtime/src/framework/input.rs:397
⑥ 命中与焦点  nana-ui-runtime     查询增量命中索引（world/hit_test.rs）与焦点链定位目标节点
⑦ 触发监听器  nana-ui-runtime     触发 NodeStore 的 event_listeners 侧表（event_flags 权威所在）
⑧ 应用回调    应用                cx.on(button, |_, Activate, cx| …) 或 .on_activate(...)
```

关键要点：

- 无头测试走 `HeadlessInput::route_shaped`（`crates/nana-ui-runtime/src/framework/input/headless.rs:119`），直接向同一个 `route_input` 注入确定性的输入序列。
- `nana-ui-devtools` 的 Agent 会话复用同一条无头通道（`agent/runtime.rs:120`），因此自动化测试观测到的输入行为与生产窗口完全一致。

## 流程三：一段文字如何变成屏幕像素

场景：`Text` 组件显示一段字符串，从样式与文本源一直到字形图集（Glyph Atlas）上屏。

```text
① 文本源      nana-ui-core        字符串 + TextStyle（TextKind、字号、变体轴、富文本跨度）构成 TextSource
② 引擎入口    nana-text           TextEngine::layout(kind, source, base, constraints, counters) → Arc<TextLayout>
                                   crates/nana-text/src/engine.rs:36
③ 文本整形    nana-text           Shaper::shape(fonts: &mut FontSystem, request: &ShapeRequest) → Arc<ShapedText>
                                   crates/nana-text/src/shaping/shaper.rs:116
                                   （HarfRust OpenType 整形位于 shaping/opentype.rs:73）
④ 段落排版    nana-text           Layouter 按 TextConstraints 折行与截断（省略号同样经标准路径整形），
                                   产出不可变的 TextLayout（crates/nana-text/src/layout/ir.rs:182）
⑤ 分级缓存    nana-text           同文本同约束直接返回缓存中的同一个 Arc<TextLayout>；
                                   仅宽度变化时重跑第 ④ 步而不重跑第 ③ 步整形；
                                   FontGeneration 每次字体集变更 +1，自动作废旧代际布局
⑥ 进程级共享  nana-ui             SharedTextEngine = Arc<Mutex<NativeTextEngine>>
                                   crates/nana-text/src/engine.rs:50
                                   全进程共享唯一引擎实例：一个标签的整形结果可供所有副本直接复用
⑦ 布局测量    nana-ui-runtime     RuntimeLayoutEngine 在布局阶段向文本引擎查询固有尺寸，
                                   并将生成的 TextLayoutId 世代句柄留在节点上（TextLayoutStore）
⑧ 栅格化上屏  nana-ui-scene → ui  保留句柄随 ExtractedNode 进入 UiScene；
                                   NanaRenderer::text 直接绘制保留布局，将字形栅格化进多页 GPU 图集上屏
```

关键要点：

- 文本布局是**保留式（Retained）**的：不是每帧从字符串重排，而是按句柄与缓存键跨帧复用；纯颜色、不透明度或几何变换改动完全不触碰文本引擎。
- 可变字体轴动画（`font_axis` / `font-variation-settings`）编译为逐轴轨道，按 `SHAPE_STYLE` 脏图驱动真实字形重新整形与栅格化，绝不用几何缩放伪造。

## 流程四：一块 GPU 实时画面如何挂到树上

场景：应用将一段着色器渲染结果或视频流放进界面，要求它像普通按钮一样参与布局、受圆角裁剪、可被上层弹窗遮挡、可响应点击。

```text
① 画面生产    应用 / frame-exchange  将画面画到可采样 GpuTexture（跨线程生产者经 nana-frame-exchange 传递，
                                     不拥有 UI 设备或提交权）
② 槽位登记    nana-ui                HostTextureRegistry::slot("preview") → TextureSlot
                                     crates/nana-ui/src/gpu_texture.rs:555, 766
③ 挂载节点    nana-ui-runtime        在界面树上挂载 GpuTextureView::new("preview")（模板标签 <Texture>）
                                     crates/nana-ui-runtime/src/gpu_slots.rs:203
④ 树的待遇    nana-ui-runtime        与 Button 完全一致：参与 Flexbox/Grid 布局、受父级圆角与 overflow 裁剪、
                                     进入增量命中索引、按文档顺序参与遮挡
⑤ 更新通知    nana-ui                纹理内容更新时通过 TextureSlot 仅唤醒引用了该槽位的窗口；
                                     窗口 0 尺寸时仍执行 prepare，尺寸可画时才编码 producer
⑥ 场景抽取    nana-ui-scene          flush 时作为 CustomRenderNode（renderer = "nana.host-texture"）进入 UiScene
⑦ 文档序绘制  nana-ui                SceneWgpuPainter::paint_target 在当前目标 pass 内按文档顺序就地采样；
                                     多层实时画面只需并排挂载多张 GpuTextureView
⑧ 生命周期    nana-ui                直到 window_frame_presented（流程一第 ⑨ 步）才释放上一帧被替换的旧纹理；
                                     更换纹理尺寸或对象时递增 generation，不销毁重建 Runtime 节点
```

关键要点：

- 两条 GPU 内容接入路径：有中间纹理的默认路径使用 `GpuTextureView`（字符串槽位名）；无中间纹理、需直接编码进当前 UI pass 的高级路径使用 `GpuView`（`u64` 槽位号，配合显式注册的 `SceneGpuRenderer`）。
- 声明与执行解耦：GPU 槽位声明位于 `nana-ui-runtime`（不依赖 WGPU），实际管线采样与录制位于 `nana-ui`。

## 四条数据流的共同结构

把上面四条流叠在一起，可以看到 NanaUI 全仓一致的五层漏斗模型：

| 阶段 | 所在层 | 结构特征 |
| --- | --- | --- |
| **1. 外部输入 / 生产** | 最外层（应用、操作系统） | 形态各异：原生窗口事件、字符串、样式文本、GPU 纹理 |
| **2. 规范化降维** | 边界 crate（`input` / `text` / `gpu` / `css`） | 在边界处一次性翻译为框架合同：`CanonicalInputEvent`、`TextSource`、`TextureSlot`、`LayoutStyle` |
| **3. 权威汇合** | `nana-ui-runtime`（`UiWorld`） | 全部汇合为保留树上的节点状态与原子 `UiMutation` |
| **4. 场景投影** | `nana-ui-scene`（`UiScene`） | 单次有界 `flush` 事务完成布局、命中、无障碍与绘制图增量更新 |
| **5. 帧内呈现** | `nana-ui`（`SceneWgpuPainter`） | 按文档顺序录制进宿主拥有的 `FrameContext` 并提交上屏 |

**边界 crate 只做翻译，状态权威只在 `UiWorld`，上屏呈现只在 `SceneWgpuPainter`。** 抓住这句话，阅读任何模块都能立刻判断它处于哪一层、该向谁负责。

## 接着读

- [保留树：UiWorld](/learn/runtime-tree) · [Scene 提取](/learn/scene-extraction)：汇合点与绘制图投影的内部细节
- [GPU 与帧生命周期](/learn/gpu-frame)：`FrameContext`、上传环、提交守卫与 35 步重绘全序
- [框架如何运行（参考）](/reference/how-it-works)：面向应用开发者的同款运行模型
