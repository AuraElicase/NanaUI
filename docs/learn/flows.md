---
title: 端到端数据流
---

# 端到端数据流

前面各章按子系统切开讲。这一章反过来:选四条贯穿多层的链路,从头到尾走一遍。每一步都标出所在的 crate 与关键类型——这是检验你是否真的理解这个框架的最好方式。

## 流程一:一次状态变更如何变成像素

场景:应用代码改了树(或 Vue 提交了一批 host ops),到屏幕上出现新像素。

```text
① 变更入队    nana-ui-runtime   UiMutation(Create/Insert/Detach/SetStyle/…)
                                 crates/nana-ui-runtime/src/mutation.rs:15
                                 Vue 路径先攒在 PendingHostOps
                                 (crates/nana-ui-vue/src/tree.rs:220),帧边界 commit
② 帧调度      nana-ui           FrameDemand(OnDemand / 截止时间 / Continuous)
                                 crates/nana-ui/src/runtime_host.rs:1035
                                 宿主按需求决定这帧画不画;没有变更 flush 空转
③ 消息消化    nana-ui           run_runtime_scene 把 dispatch_program 的消息
                                 交给 RuntimeProgram::update(开窗/换 GPU/持久化)
                                 crates/nana-ui/src/scene_host.rs:99
④ 备纹理      nana-ui           prepare_window_frame:宿主把最新纹理准备好
⑤ flush       nana-ui-scene     RuntimeDocument::flush:样式 → 文字 → 布局 →
                                 命中 → 抽取,一次做完
                                 crates/nana-ui-scene/src/runtime_document.rs:109
⑥ 取 Surface  nana-ui           获取窗口 Surface;外部资源生产编进 FramePlan,
                                 与 UI 同一宿主 encoder,失败整帧丢弃
⑦ 绘制        nana-ui           SceneWgpuPainter::paint_target:可见操作按文档
                                 顺序画进目标
                                 crates/nana-ui/src/scene_paint/mod.rs:899
⑧ 提交        nana-gpu          queue.submit + submitted + present,每目标一份
                                 NanaUI 提交(FrameContext 管理 encoder 生命周期)
⑨ 收尾        nana-ui           window_frame_presented:现在才能丢上一帧纹理;
                                 bind_window 按需再填内容
```

要点:

- `RuntimeDocument` 定义在 **nana-ui-scene**(`crates/nana-ui-scene/src/runtime_document.rs:18`),它同时持有 `AppContext`(树)与 `UiScene`(绘制图)——flush 就是把前者推进后者。
- 动画与实时 GPU 画面的唤醒与普通 UI 分开:`UiScene::compositor_needs_tick` 只把该窗口接到轻量 `FrameDemand::Continuous` present,不强制全局 schedule。
- 被遮挡/最小化的窗口:宿主仍调 `prepare_window_frame`,但不 flush、不取 Surface、不 present。

## 流程二:一次点击如何变成回调

场景:用户按下按钮,操作系统事件到达应用回调。

```text
① 系统事件    nana-window/winit 原生窗口事件(平台相关形状)
② 降维        nana-ui-input     宿主把它降维成 CanonicalInputEvent
                                 (InputPayload / KeyInput / PointerInput,
                                 crates/nana-ui-input/src/canonical.rs)
③ 排序        nana-ui-input     InputSequencer 盖序列号、维持规范顺序
                                 (crates/nana-ui-input/src/canonical.rs:333)
④ 服务合同    nana-ui-input     HostServices:剪贴板、IME 等宿主能力经此注入
                                 (crates/nana-ui-input/src/host_services.rs:97)
⑤ 路由        nana-ui-runtime   AppContext::route_input —— 全仓唯一定义
                                 crates/nana-ui-runtime/src/framework/input.rs:397
⑥ 命中+焦点   nana-ui-runtime   用 flush 阶段维护的命中结构与焦点链决定目标节点
⑦ 监听器      nana-ui-runtime   Runtime 事件监听器(event_flags 在此层)触发
⑧ 应用回调    应用 / nana-ui-vue Rust:cx.on(button, |_, Activate, cx| …)
                                 Vue:observe_routed_input 翻译成页面事件
                                 (crates/nana-ui-vue/src/hosted_adapter.rs:446)
                                 —— Vue 观察的是已路由结果,不是第二套路由
```

要点:

- headless 测试走 `HeadlessInput::route_shaped`(`crates/nana-ui-runtime/src/framework/input/headless.rs:119`),注入已成型的输入序列。
- devtools 复用同一通道(`agent/runtime.rs:120` 调 runtime HeadlessInput),所以工具看到的输入语义与生产一致。

## 流程三:一段文字如何变成像素

场景:`Text` 组件显示一个字符串。

```text
① 输入        nana-ui-core      字符串 + TextStyle(TextKind、字号、字体轴、
                                 跨度)构成 TextSource
② 引擎合同    nana-text         TextEngine::layout(kind, source, base,
                                 constraints, counters) → Arc<TextLayout>
                                 crates/nana-text/src/engine.rs:36
③ 整形        nana-text         Shaper::shape(fonts: &mut FontSystem,
                                 request: &ShapeRequest) → Arc<ShapedText>
                                 crates/nana-text/src/shaping/shaper.rs:116
                                 (OpenType 整形:shaping/opentype.rs:73)
④ 排版        nana-text         Layouter 按 TextConstraints 折行/截断(省略号
                                 也是普通字符,同样整形)产出 TextLayout
⑤ 缓存        nana-text         布局不可变:同文本同约束返回同一个 Arc;
                                 FontGeneration 每次字体集变更 +1,旧代际布局作废
⑥ 共享        nana-ui           SharedTextEngine = Arc<Mutex<NativeTextEngine>>
                                 crates/nana-text/src/engine.rs:50
                                 全进程一个引擎:一个 label 的整形结果,
                                 其他副本直接复用
⑦ 布局消费    nana-ui-runtime   RuntimeLayoutEngine 布局时向文本要固有尺寸
                                 (how-it-works:flush 调宿主文字整形)
⑧ 绘制        nana-ui-scene→ui  文本进 UiScene;painter 对 glyph 栅格化上屏
```

要点:

- 文本是**保留布局**:不是每帧重排,而是按(文本,约束)缓存;这就是 AGENTS.md 说 nana-text 是「产品文本测量、整形与保留文本布局权威」的含义。
- 字体轴动画(`font_axis`)按脏图重新 shaping,与 CSS `font-variation-settings` 编译成同一种逐轴 track。

## 流程四:一块 GPU 实时画面如何挂到树上

场景:应用把一段着色器输出或一个视频放进界面,它要能被布局、裁剪、遮挡、点击。

```text
① 生产        应用/nana-frame-exchange   画面画到可采样纹理;生产者帧经
                                    nana-frame-exchange 运输(它不拥有设备)
② 登记        nana-ui               HostTextureRegistry::slot("同一字符串 slot")
                                    → TextureSlot
                                    crates/nana-ui/src/gpu_texture.rs:555,766
③ 挂树        nana-ui-runtime       树上挂 GpuTextureView(同一字符串 slot)
                                    crates/nana-ui-runtime/src/gpu_slots.rs:203
④ 树的待遇    nana-ui-runtime       与 Button 完全一样:参与布局、被裁剪、
                                    可命中、按文档序合成——它不是洞也不是覆盖层
⑤ 更新通知    nana-ui               纹理内容更新经 TextureSlot 通知引用该资源
                                    的窗口;0 维仍然 prepare,尺寸可画才 encode
⑥ 提取        nana-ui-scene         flush 时进 UiScene(GPU 内容用
                                    CustomRenderNode 或普通 HostTexture 节点表示)
⑦ 绘制        nana-ui               paint_target 按文档序画进宿主的帧;
                                    多层画面就是相邻几张 GpuTextureView
⑧ 生命周期    nana-ui               present 之后才丢上一帧纹理(流程一步⑨),
                                    换纹理升 generation,不拆节点
```

要点:

- 两条 GPU 内容通道:`GpuTextureView`(有中间纹理的默认做法)与 `GpuView`(`u64` slot,没有中间纹理时;它不是 registry 键)。
- 高级路径 `CustomRenderNode`:GPU 槽位声明在 Runtime(`crates/nana-ui-runtime/src/gpu_slots.rs`),绘制管线在 nana-ui——声明与执行分离,树始终完整。
- `<video data-nana-video>` 走 `video:{id}` 这条 HostTexture,有槽时不叠 poster。

## 四条流的公共结构

把四条流叠起来看,会看到同一个形状:

| 阶段 | 所在层 | 特征 |
| --- | --- | --- |
| 生产/输入 | 最外层(应用、OS、JS) | 形状各异:事件、字符串、纹理 |
| 规范化 | 边界 crate(input/text/gpu/css) | 降维成框架合同:CanonicalInputEvent、TextSource、TextureSlot |
| 汇合 | nana-ui-runtime(UiWorld) | 全部变成树上的节点与变异 |
| 投影 | nana-ui-scene(UiScene) | 布局/命中/抽取,一次 flush |
| 呈现 | nana-ui(painter) | 按文档序画进宿主拥有的帧 |

**边界 crate 只做翻译,权威只在 UiWorld,呈现只在 painter。** 记住这句,新读任何模块都能定位它站在哪一层、该向谁负责。

## 接着读

- [保留树](/learn/runtime-tree) / [Scene 提取](/learn/scene-extraction):汇合点与投影的内部
- [GPU 与帧](/learn/gpu-frame):FrameContext 与提交守卫
- [框架如何运行](/reference/how-it-works):应用作者视角的同一张图
