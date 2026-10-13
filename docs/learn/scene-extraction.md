---
title: Scene 提取
---

# Scene 提取:从 UiWorld 到绘制图

这一章下钻 `nana-ui-scene`:**保留树的一批脏节点如何变成一份可增量更新的绘制图 `UiScene`**。读完你能搞懂:`ExtractedNode` 里装了什么、`ScenePrimitive` 的四种坐标信息(位置、变换、裁剪链、顺序)分别由谁维护;`apply_delta` 的增量失效规则为什么这么设计;`FramePlan` 帧图如何在不依赖任何 GPU 类型的前提下描述一帧的绘制程序;以及命中测试、无障碍为什么不在这一层。它承接[运行时保留树](/learn/runtime-tree)的 flush 结论,是 [GPU 与帧生命周期](/learn/gpu-frame)的上游。

## 职责与边界

- **中立绘制权威**:`nana-ui-scene` 的 crate 文档自述为 "Backend-neutral render scene and frame graph",并且 "owns no application state, window, GPU device, or renderer objects. Product paint is `SceneWgpuPainter` in `nana-ui`"(`crates/nana-ui-scene/src/lib.rs:1`)。它把 `ExtractedNode` 变成带顺序的 `ScenePrimitive` 集合,自己不画。
- **为什么不依赖 WGPU**:不是「还没接」,是合同。绘制图的全部输出(`ScenePrimitive`、`FramePlan`、`RenderGraph`)都是纯数据:没有纹理句柄、没有 pass 对象、没有格式枚举;`FramePlan` 的类型注释专门强调资源内容与绑定在消费时才解析("a texture update does not compile a graph",`composition.rs:8`)。于是换后端、做无 GPU 的截图/测试、把 scene 序列化做回放,都不需要动这一层——WGPU 的知识被完整压在 painter 一侧(见 [GPU 与帧生命周期](/learn/gpu-frame))。
- **增量为生**:整棵 `UiScene` 是保留结构——原始图形按 `PrimitiveId` 存在 `BTreeMap` 里,每帧只对脏子树重建,滚动这类高频变化走纯平移快路径(`crates/nana-ui-scene/src/scene.rs:1672`)。
- **边界之外**:不解析 CSS(runtime 也不解析,见[模块与分层](/learn/modules-and-layers));不碰 WGPU(依赖表里没有 `nana-gpu`);**不做命中测试与无障碍**——两者都由 `UiWorld` 在 flush 里完成(见「协作与数据流」);文本整形与布局也都在上游 runtime 内做完,scene 只消费 `LayoutBox` 与 `RetainedTextLayout`。
- **一个例外入口**:`RuntimeDocument` 定义在这里(`crates/nana-ui-scene/src/runtime_document.rs:18`),它用 runtime 的 `AppContext` 驱动整帧 flush 并持有 `Arc<UiScene>`——所以 scene **依赖** runtime,而不是反过来。

## 依赖与邻居

层级:core(L1)→ text/css/charts/input/window/gpu(L2)→ platform/frame-exchange(L3)→ runtime(L5)→ **scene(L6)**→ nana-ui(L7)。scene 依赖 `nana-ui-runtime`、`nana-ui-core`(motion GPU 类型经 lib.rs 再导出),**不依赖** `nana-gpu` 与 `nana-ui-css`。

| 邻居 | 关系 |
| --- | --- |
| `nana-ui-runtime` | `UiWorld::extract_nodes` 产出 `ExtractedNode`(`crates/nana-ui-runtime/src/world/extraction.rs:652`);`NodeKind`/`LayoutBox`/`ComponentGeometry`/`CustomRenderNode` 等类型由 runtime 定义 |
| `nana-ui` | `SceneWgpuPainter` 消费 `UiScene` 的原语与 `frame_plan()`,见 [GPU 与帧生命周期](/learn/gpu-frame) |
| `nana-ui-core` | `ScenePrimitive` 的底层几何与 motion GPU 数据 |
| `nana-text` | 只间接:文本布局结果已经装进 `ExtractedNode.text_layout`(`crates/nana-ui-runtime/src/components.rs:4198`,Issue #95) |
| `nana-diagnostics` | flush 以 `FrameStage::Extract` 计时;8-pass 超限打 `FLUSH_DID_NOT_SETTLE` 指标(`runtime_document.rs:203`) |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `ExtractedNode` | `crates/nana-ui-runtime/src/components.rs:4181` | 抽取产物:一个节点绘制所需的全部快照(kind/style/layout/text/…) |
| `UiScene` | `crates/nana-ui-scene/src/scene.rs:1357` | 保留绘制图:节点表 + 原语表 + 顺序表 + 各类缓存,`instance` 计数器单调 |
| `ScenePrimitive` | `crates/nana-ui-scene/src/scene.rs:1257` | 单条绘制原语:id、bounds、transform、共享裁剪链、paint_opacity、z_index、document_order、kind |
| `ScenePrimitiveKind` | `crates/nana-ui-scene/src/scene.rs`(枚举) | quad/mesh/icon/text/chart/host_texture/custom 等具体绘制类别 |
| `PrimitiveId` | `crates/nana-ui-scene/src/scene.rs:145` | `(StableNodeId, slot: u64)`;一个节点可占多个槽位(文字、选区、滚动条各一) |
| `AffineTransform` | `crates/nana-ui-scene/src/scene.rs:62` | 2×3 仿射 + 射影行,支持 3D |
| `ClipRegion` | `crates/nana-ui-scene/src/scene.rs:114` | 裁剪盒 + 自身变换 + 圆角/多边形裁剪 |
| `SceneOrderKey` | `crates/nana-ui-scene/src/scene.rs:1299` | 全序键:栈组前缀 + paint_layer + slot + node |
| `SceneDelta` | `crates/nana-ui-scene/src/scene.rs:1315` | 一次 `apply_delta` 的增量报告(added/removed/paint/transforms/clips/order_changed + stats) |
| `SceneDeltaStats` | `crates/nana-ui-scene/src/scene.rs:1334` | 计数:updated/removed 节点数、重建原语数、是否重排序、原语总数 |
| `DrawAttributes` / `SceneDraw` | `crates/nana-ui-scene/src/scene/attributes.rs:5` / `:13` | 增量问答层:继承属性缓存与单次绘制答案(见内部结构) |
| `RuntimeDocument` | `crates/nana-ui-scene/src/runtime_document.rs:18` | AppContext + DocumentId + `Arc<UiScene>`,规范宿主的帧事务 |
| `RuntimeFrameUpdate` | `crates/nana-ui-scene/src/runtime_document.rs:26` | 一次 flush 的结果:passes、cursor、scene delta、a11y delta |
| `FramePlan` | `crates/nana-ui-scene/src/scene/composition.rs:8` | 结构化帧程序:`RenderOperation` 列表 + preparations + custom_nodes |
| `RenderGraph` / `CompiledRenderGraph` | `crates/nana-ui-scene/src/graph.rs` | 传递闭包 DAG,`compile()` 做拓扑排序与冒险检测 |
| `RenderOperation` | `crates/nana-ui-scene/src/graph.rs` | `PrepareExternal` / `Draw` / `InvokeCustom` 三类节点操作 |
| `VisibilityIndex` | `crates/nana-ui-scene/src/scene/visibility.rs:6` | 有序包围盒层级:平移/保守包含,射影即放弃 |

## 内部结构

### 抽取:谁在什么时机产出 ExtractedNode

抽取的发起者不是 scene,而是 `RuntimeDocument::flush`(`crates/nana-ui-scene/src/runtime_document.rs:109`)。flush 是一个有界循环 `flush_loop`(`runtime_document.rs:174`):每轮 `take_system_work` 排干系统工作,直到为空;上限 `MAX_FRAME_PASSES = 8`(`runtime_document.rs:11`),超限报 `FrameworkError::FrameDidNotSettle` 并打 `FLUSH_DID_NOT_SETTLE` 指标——一帧必须收敛,不收敛就整帧作废重试。循环内依次跑 `resolve_styles` → 文本整形 → 布局(`layout_document_for_viewport` / `layout_document_with_frontier`),随后至多 `POST_LAYOUT_SHAPE_PASSES = 4`(`runtime_document.rs:48`)轮「布局后重整形→重布局」直到几何稳定;viewport 只在稳定后才发布——失败帧以同一 viewport 重试,不会把中间态漏给布局。

为什么形状-布局要单独限轮:源码注释写得很直白——"enough for a wrap to widen a box that widens another wrap, bounded so a layout that never settles cannot hold the frame"(`runtime_document.rs:45`):换行会撑宽盒子、盒子又撑出新的换行,四轮覆盖这条链,永不收敛的布局不许拖住整帧。主循环的 8-pass 界防的则是「系统工作再生系统工作」——`RuntimeDocument` 的类型文档把这称为同一个有界帧事务:"this owner guarantees text, layout, hit-test, accessibility, and render extraction are drained in the same bounded frame transaction"(`runtime_document.rs:15`)。事务的同一性还有个指纹:`RuntimeFrameUpdate.layout_generation` 是本次 flush 的规范布局修订号,"shared by the Scene, hit-test and accessibility projections for this flush... so paint-only work keeps it stable"(`runtime_document.rs:28`)——绘制、命中、a11y 三个投影消费同一份布局,只有画面的那一帧不推动它。

循环排干后,flush 收尾四件事(`runtime_document.rs:264`–330):命中测试工作(`apply_hit_test_work`)、无障碍投影(`project_accessibility_delta`)、**渲染抽取**、以及(有 Compositor 动画时)`apply_presentation`。渲染抽取这一步是:

- `self.context.world().extract_nodes(&render_dirty)` —— `UiWorld::extract_nodes(&self, ids: &[StableNodeId]) -> Vec<ExtractedNode>`(`crates/nana-ui-runtime/src/world/extraction.rs:652`)。文档注释写明只抽脏节点,"Hidden nodes stay present with `visible=false` so an incremental renderer can remove their previous primitives"——隐藏不等于删除,增量渲染器靠它撤掉旧原语。全量快照另有 `extract_document(document)`(`extraction.rs:665`),按保留文档序遍历。
- `Arc::make_mut(&mut self.scene).apply_delta(extracted, render_removed)` —— 写时复制:没人共享 scene 时原地改,有共享快照(`shared_scene()`,`runtime_document.rs:88`)时才克隆。注释明确计时归入 `FrameStage::Extract`:"apply_delta is part of Extract"(`runtime_document.rs:291`),因为它走节点、重建原语、做写时复制,不是廉价赋值。

`ExtractedNode`(`crates/nana-ui-runtime/src/components.rs:4181`)的形态值得看一遍,它决定了 scene 能做什么:`kind: Arc<NodeKind>` 与 `style: Arc<ComputedStyle>` 与 UiWorld **共享**(注释:"Idle extract is an Arc bump, not a `NodeKind` clone",`components.rs:4183`)。`component_geometry: Option<Box<ComponentGeometry>>` 装箱是因为 "the `TextInput` variant alone is ~1.8 KB"(`components.rs:4222`)。`chrome_radii: ChromeRadii` 把主题解析过的圆角带下来,注释解释了为什么:"Before this the Scene read the `UI_METRICS` constant and a themed radius could not reach menu surfaces, modal frames or palette rows"(`components.rs:4234`)。`compositor: ExtractedCompositor` 只携带提升提示,"Logical style stays on `source_style`; presentation is not baked in"(`components.rs:4245`)。`custom_paint: Option<Arc<crate::PaintRecording>>` 是应用 `Painter` 的录制(Issue #217),None 即走内建绘制路径。

### 节点种类:从 NodeKind 到 ScenePrimitiveKind

`UiScene` 自己不存 `NodeKind`,它把每个 `ExtractedNode` 展开成一至多条 `ScenePrimitive`(`crates/nana-ui-scene/src/scene.rs:1257`),按 `PrimitiveId{node, slot}`(`scene.rs:145`)登记。槽位编号有约定:节点自身用固定小槽 0..=255,集合类内容用 `namespace << 32 | index`(`collection_slot()`,`scene.rs:152`)。命名空间常量在 `scene.rs:161`–191:文本行标签(1)、诊断标记(2)、诊断标签(3)、原子图标(4)、原子标签(5)、**文档文本选区(6)**、内联对象(7)、编辑器光标(8)、选择菜单背景/标记/标签/提示/滚动条(9–13)等;富文本编辑器还有 `RICH_EDITOR_SELECTION_BASE = 1<<16`、`RICH_TAG_BASE = 1<<17`。两个全局槽要单独记:

- `TRIGGERED_OVERLAY_SURFACE = 0xFFFF_FFE0`(`scene.rs:216`):popover/menu 的表面画进**根层叠上下文**,不被触发者的裁剪链剪裁;
- `SCROLLBAR_CHROME = 0xFFFF_FFF8`(`scene.rs:226`):滚动条槽位排在所有后代之后,不被内容压住;
- `MODAL_SCRIM_SLOT = 10`(`scene.rs:208`):模态遮罩,与 `ModalFrame` 的表面/主体裁剪(`ComponentGeometry` 变体)配合决定模态框内外的绘制边界。

`ScenePrimitiveKind` 的具体变体(quad 表面、文字、图表、图标、网格 mesh、host 纹理、custom 等)由 `scene/primitives/` 子模块按控件族填充(text.rs、charts.rs、controls.rs、overlays.rs、rich_text.rs、markdown_drawing.rs、image_viewer.rs、graph_canvas.rs、calendar.rs);应用自定义绘制走 `scene/custom_paint.rs`:把 `PaintRecording` 经 i_overlay 解析填充/描边/裁剪规则、lyon 曲面细分加 AA 边缘,`TOLERANCE = 0.05` 逻辑像素、`MITER_LIMIT = 4.0`(`crates/nana-ui-scene/src/scene/custom_paint.rs:45`–48);生成 `PathMesh{vertices, indices, bounds, gradient?}`(`custom_paint.rs:52`),几何按节点保留——移动或滚动只重发不重分。

### 变换:AffineTransform 与祖先链合成

`AffineTransform`(`crates/nana-ui-scene/src/scene.rs:62`)存 `[f32; 6]`(2×3)加 `[f32; 2]` 射影行,因此覆盖 3D 变换;`is_projective()` 以 `|w| > 1e-8` 判定(`scene.rs:71`),`then()` 合成时若底行接近零则退化为恒等——注释解释这是刻意把 CPU 合同放宽到与 WGSL 的 f32/FMA 舍入对齐(`scene.rs:75`)。每条 `ScenePrimitive` 的 transform 是**从根到自己**的完整祖先合成,由 `compute_ancestor_state` 完成(`crates/nana-ui-scene/src/scene.rs:2176` 起的第二 impl 块):它从根向叶子走一遍祖先,沿途累乘非隔离开启节点的 opacity(注释:opacity 烘进后代原语,`OpacityGroup` 才隔离);几何链从最近的 `PositionSpec::Fixed` 祖先重新起算(fixed 脱离外层滚动/变换/裁剪链但保留结构性 opacity);每个祖先贡献 `overflow_clip_region`、`ComponentGeometry::EmptyState{root_clip}`、`ModalFrame` 的表面/主体裁剪,以及 `clip_path_region`;**然后**才 `transform.then(scroll_offset)`——滚动平移在裁剪之后乘,所以滚动自己的内容不被自己的 overflow 裁掉。文本呈现动画的 inline object 由 `present_inline_objects`(`scene.rs:2392`)按 CPU 采样搬移并递增 scene `instance`。

### 裁剪:共享裁剪链

`ClipRegion`(`crates/nana-ui-scene/src/scene.rs:114`)带自己的变换(所以斜切父容器也能正确裁圆角子元素)、`corner_radius`、`polygon_clip`;`axis_aligned()`(`scene.rs:124`)是快速路径,`ellipse()`(`scene.rs:134`)用哨兵 `polygon_clip=[[-inf,-inf]]` 表示椭圆。每条 `ScenePrimitive` 的 `clips: Arc<[ClipRegion]>` 是**共享的祖先裁剪链**(`scene.rs:1257`):同一条链上的所有原语共享一次 `Arc` 分配,而不是每条原语复制一份——这是 scene 内存结构里最直接的省法。渲染侧(见 [GPU 与帧生命周期](/learn/gpu-frame))会把链合成成物理像素 scissor,交给各 pass。

### 顺序:文档序、栈组与两个全局覆盖

顺序的权威是 `SceneOrderKey`(`crates/nana-ui-scene/src/scene.rs:1299`):`stack`(外层→内层每层隔离开启组的 `(z_index, document_order)` 前缀,最后接节点自身)、`paint_layer`、`slot`、`node`。它保证三件事:opacity 组、`isolation: isolate`、定位+z-index 的子树**连续**(不被外层内容穿插);`position: fixed` 画进根层叠上下文;注释老实承认这不是完整 CSS 附录 E。排序实现 `sort_primitives`(`crates/nana-ui-scene/src/scene/order.rs:6`)按节点缓存 `GroupPrefix` 的 `Arc`;「自身绘制又有子树」的节点(如滚动视图的滚动条)经 `painted_prefixes` 拿到独立前缀;`root_layer_key()`(`scene.rs:289`)把 triggered overlay / select menu 换到根层。文档序本身由 `rebuild_document_order`(`order.rs:72`)重建:根按 `StableNodeId` 排序,游离节点**追加**保留——注释说明这样不完整的增量永不丢保留节点。

### apply_delta:增量失效规则

`UiScene::apply_delta(extracted, removals) -> SceneDelta`(`crates/nana-ui-scene/src/scene.rs:1672`)是 scene 的写入口。规则按代价从廉到贵:

1. **滚动快路径**:仅滚动变化且 transform 非射影 → 对应原语做线性部分映射的平移(`scroll_translations`);射影变换则升级为子树重建并作废可见性索引。
2. **普通增量**:`delta.paint` 恒记;layout/scroll_offset 变 → `transforms`;继承性裁剪变化 → `clips`;层级(parent/children/position)变化 → `hierarchy_changed`。
3. **顺序重建**:`paint_order_facts(old) != paint_order_facts(new)`(z-index 与每个后代键里的组开启者)→ `stacking_changed`,全量重排序。
4. **子树重建**:继承 opacity 的位模式变化 → `subtree_rebuild`(opacity 已烘进后代原语,组隔离才救得回来),重建集合还要并入「重建根之下未被抽取的后代」与继承根之下的不可调投影。
5. **结构变化**则 `frame_plan.take() + visibility.take()`,否则可见性索引做子树平移/刷新。

每条保留原语带 `build: u64` 戳(`RetainedPrimitive`,`scene.rs:1344`):最后一次写它的重建代号;`UiScene.instance` 在任何改节点的变异后前进且永不为零——painter 用它校验操作流的代际。调试构建里有 `audit_retained_projection()`(`scene.rs:1950`):小树上从零重算顺序/计划/可见性再对比,让既有测试顺带检验失效规则本身。

### 增量问答层:DrawAttributes 与 SceneDraw

画家侧经常要问「这条原语这帧变没变」。`UiScene` 把答案做成两层(`crates/nana-ui-scene/src/scene/attributes.rs`):`DrawAttributes{epoch, delta, parent_clips}`(`attributes.rs:5`)按节点缓存继承结论(裁剪链、继承变换),`epoch` 对 `attribute_epoch`——祖先继承一变就整体作废;`SceneDraw<'a>{primitive, &transform, clips, paint_opacity, revision}`(`attributes.rs:13`)是单次绘制问询的打包答案,其中 `revision` 就是 `RetainedPrimitive.build` 戳。类型文档点明分工:"A painter that keeps its own resolved copy can answer from it instead of deriving the question again"——画家自持已解析副本时可以直接回话,不必重推问题。判定「变换是否真的变了」用 `same_drawn_transform`,容差 1e-4 / 0.05px(`attributes.rs:39`):sub-pixel 级抖动不算变化,painter 可以继续吃缓存。与 `document_access.rs` 的 `DocumentAccessError{Busy, "document is already borrowed", Poisoned}`(`crates/nana-ui-scene/src/document_access.rs`)合起来,就是 scene 对外暴露的全部「读侧」表面:同步借用失败就报错,不阻塞、不等待。

### 帧图与可见性:FramePlan 的结构化输出

`UiScene::frame_plan()`(`crates/nana-ui-scene/src/scene/composition.rs:15`)把绘制图压成一份**结构化帧程序** `FramePlan{operations, preparations, custom_nodes}`(`composition.rs:8`),缓存在 `OnceLock` 里。类型注释点明设计:"Resource contents and bindings are deliberately resolved at consumption time, so a texture update does not compile a graph"——资源内容与绑定刻意在消费时才解析,纹理更新不触发重编译。构建走 `build_frame_plan`(`composition.rs:27`)→ `frame_graph(target)`(`composition.rs:106`):target 作为外部资源 `"ui-target"`;**每个渲染器**一个 prepare pass(不是每个资源——注释:compile() 对 pass 数是二次的);按场景序走原语,Custom 节点冲掉标准 run、把同 renderer 的 custom run 连成 `custom:{renderer}` pass,标准原语回到 `ui-standard` pass——场景序不动,custom run 只被下一个标准原语或不同 renderer 截断。若两个 Custom 节点以不同 (renderer, revision) 声称同一个 host-texture 槽,`conflicting_external_resource()`(`composition.rs:58`)在画之前拒绝整帧(`GraphError::ConflictingExternalResource`)。

底层 DAG 在 `graph.rs`:`RenderGraph::compile()`(`crates/nana-ui-scene/src/graph.rs:141`)校验资源与依赖、用 `has_hazard`(同资源且任一方写 → 后者依赖前者,`graph.rs:245`)补冒险边,Kahn 拓扑排序(低索引优先),成环报 `GraphError::Cycle`。配套的 `VisibilityIndex`(`crates/nana-ui-scene/src/scene/visibility.rs:6`)是有序包围盒层级:文件头注释先立规矩——"A missing bound means conservative inclusion; it never means an invisible primitive or permission to reorder operations"(`visibility.rs:1`);射影变换的包围盒返回 None 走保守包含(`visibility.rs:97`)。合成器层(`scene/compositor.rs`)有滞回:`LAYER_PROMOTE_HOLD = 16ms`("Sub-frame overlay flashes never promote",`compositor.rs:20`)、`LAYER_DEMOTE_HOLD = 120ms`(`compositor.rs:23`),`COMPOSITOR_PROPERTIES` 只认 Transform/Opacity/Clip/ShaderParameter(`compositor.rs:25`)——与[运行时保留树](/learn/runtime-tree)里「compositor 不回写基态」的合同互为表里。层的身份 `CompositorLayerId(u64)` 直接取提升节点的 `StableNodeId`(`compositor.rs:34`);动画绑定 `CompositorMotionBinding{track_id, index, generation}` 记下 MotionHandle 的轨道、下标与代际(`compositor.rs:53`),编码进 `CompositorPaintEncode{transform, transform_origin, opacity, motion_ids}`(`compositor.rs:82`)供 GPU 侧按轨道采样——逻辑样式留在 `source_style`,呈现采样不回写,正是合同那句 "presentation is not baked in"。查询面由 `opacity_groups`(`scene.rs:1603`)/`filter_groups`(`scene.rs:1661`)给隔离组、`node_bounds`(`scene.rs:1579`)给节点包围盒、`primitives()`(`scene.rs:1568`)给保留原语视图。

## 协作与数据流

一次典型帧里 scene 的位置(承接[架构帧文档](/architecture/frame)的 8 步,细节下钻见 [GPU 与帧生命周期](/learn/gpu-frame)):

1. 宿主调度 `RuntimeDocument::flush(viewport, shaper)`(输入 dispatch 与布局在 flush 内完成,见上文);
2. flush 内部:style → text → layout 循环收敛(≤8 pass),随后**同一次事务里**做命中测试与无障碍投影;
3. `world().extract_nodes(&render_dirty)` → `apply_delta` 得 `SceneDelta`,装进 `RuntimeFrameUpdate.scene`;
4. 宿主拿 `shared_scene()` 的 `Arc<UiScene>` 快照,交给 painter;painter 依据 `frame_plan()`、`SceneOrderKey` 全序与裁剪链录制 GPU 命令。

**命中测试与无障碍在哪一层**:在 runtime 的 `UiWorld`,不在 scene。flush 收尾调用的 `apply_hit_test_work` 最终落到 `UiWorld::hit_test`(`crates/nana-ui-runtime/src/world/hit_test.rs:1447`)与 `AppContext::rebuild_hit_test_for` / `update_hit_test_scroll`(`crates/nana-ui-runtime/src/framework/frame.rs:637`/`661`)——纯滚动走增量滚动更新,几何脏则重建;无障碍走 `project_accessibility_delta`(`crates/nana-ui-runtime/src/world/accessibility.rs:630`)。scene 只消费这些脏集合驱动重抽取;`SceneDelta` 不含命中或 a11y 数据,它们走 `RuntimeFrameUpdate.accessibility` 单独一条通道。这就是「命中/a11y 在 runtime、绘制图在 scene」的分工线:命中回答「点到了谁」,需要布局与几何权威;scene 回答「画什么、按什么顺序画」。

**空转也要有说法**:`RuntimeFrameUpdate::is_idle()` 就是 `passes == 0`(`runtime_document.rs:26`)——样式/文本/布局一 pass 未跑,渲染不脏时连 `extract_nodes` 都不进,`SceneDelta` 为空,宿主照常走 painter 的 dest 复用快路径。反过来 `FrameDidNotSettle` 时,flush 的错误路径已把 runtime 的脏工作还原,宿主跳过本帧的 flush 后续步骤、保留上一帧呈现,下一帧以同一 viewport 重试——`RuntimeFrameUpdate` 里没有半完成状态。光标是独立通道:`cursor_changed` 来自 `take_window_cursor_dirty`,与渲染脏不脏无关——内容没变也可能要换鼠标。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-scene/src/lib.rs` | crate 定位与模块/再导出清单 |
| `crates/nana-ui-scene/src/scene.rs` | `UiScene`/`ScenePrimitive`/顺序键/`apply_delta`/祖先合成(4100 行主体) |
| `crates/nana-ui-scene/src/scene/order.rs` | `sort_primitives`、文档序重建 |
| `crates/nana-ui-scene/src/scene/visibility.rs` | 有序包围盒层级与保守包含语义 |
| `crates/nana-ui-scene/src/scene/composition.rs` | `FramePlan`、帧图构建、外部资源冲突检测 |
| `crates/nana-ui-scene/src/scene/compositor.rs` | 合成器层提升/降级滞回与属性表 |
| `crates/nana-ui-scene/src/scene/custom_paint.rs` | `PaintRecording` → 网格(i_overlay + lyon) |
| `crates/nana-ui-scene/src/scene/attributes.rs` | `DrawAttributes`/`SceneDraw` 增量问答 |
| `crates/nana-ui-scene/src/scene/primitives.rs` | 原语填充子模块清单(text/charts/controls/overlays/…) |
| `crates/nana-ui-scene/src/scene/primitives/` | 各控件族的 `ScenePrimitiveKind` 填充 |
| `crates/nana-ui-scene/src/graph.rs` | `RenderGraph` DAG:compile、冒险边、拓扑排序 |
| `crates/nana-ui-scene/src/runtime_document.rs` | `RuntimeDocument`/`RuntimeFrameUpdate`/flush 帧事务 |
| `crates/nana-ui-scene/src/document_access.rs` | `DocumentAccessError`(Busy/Poisoned) |
| `crates/nana-ui-scene/src/icon.rs` | 图标几何与路径命令(`IconGeometry`/`IconShape`/`IconPathCommand`) |
| `crates/nana-ui-runtime/src/world/extraction.rs` | `extract_nodes`/`extract_document` 抽取入口 |

## 延伸阅读

- [GPU 与帧生命周期](/learn/gpu-frame):`UiScene` 被谁画、`FramePlan` 如何变成 pass;
- [运行时保留树](/learn/runtime-tree):flush 顺序的上游与 `SystemWork` 的来源;
- [架构:帧](/architecture/frame)与[架构:GPU](/architecture/gpu):应用作者视角的一帧 8 步;
- [代码阅读路线](/learn/reading-the-code)与[模块与分层](/learn/modules-and-layers);
- [典型数据流](/learn/flows):一帧里的输入→布局→抽取→呈现串联;
- [合同与不变量](/learn/contracts-and-invariants):保留树一份权威、投影不回写等全仓红线。
