---
title: 运行时保留树
---

# 运行时保留树:UiWorld

这一章下钻 `nana-ui-runtime` 的心脏:**UiWorld 这棵保留树(retained tree)怎么存节点、怎么接收变异、怎么把脏数据排成一帧的确定性工作**。读完你能搞懂:一个 `StableNodeId` 背后的存储长什么样;`UiMutation`/`MutationQueue` 如何以批为单位原子地改树;`ComponentRegistry` 把「语义标签 → 控件」接在哪一层;以及宿主驱动一帧时各系统(style → text → layout → hit-test → a11y → extract)的 flush 顺序与依据。它承接[模块与分层](/learn/modules-and-layers)里的层位结论,是[布局引擎](/learn/runtime-layout)、[输入链路](/learn/runtime-input)两章的共同地基。

## 职责与边界

- **唯一权威**:`UiWorld`(`crates/nana-ui-runtime/src/world.rs:628`)是全部保留节点的身份与层级权威。`lib.rs` 的模块注释即声明它是 "Backend-neutral retained runtime"(`crates/nana-ui-runtime/src/lib.rs:1`),内部节点表刻意不进入公共合同,所以换存储方案不会破坏 JS 句柄、诊断、快照或持久化数据。
- **批式变异**:外部不能直接改树,只能把 `UiMutation` 塞进 `MutationQueue`,由 `UiWorld::commit` 整批校验、整批应用(`crates/nana-ui-runtime/src/world/mutation.rs:2915`)。
- **边界之外**:布局结果只是投影(`LayoutResult` 表,见[布局引擎](/learn/runtime-layout));Vue 宿主侧的 `LayoutBoxStore`、`PendingHostOps` 同样不是权威,它们分别在 `nana-ui-scene` 与 `nana-ui-vue`(下文「投影」一节);渲染抽取产物 `ExtractedNode` 归 `nana-ui-scene` 消费。
- 运行时**不解析 CSS**:runtime 的依赖表里没有 `nana-ui-css`,样式以 Style Model 数据形式进入(见[依赖与邻居](#依赖与邻居))。

## 依赖与邻居

`nana-ui-runtime` 位于第 5 层,向下依赖 `nana-ui-core`(Style Model/主题/几何/Motion 合同)、`nana-text`(文本测量整形)、`nana-ui-charts`、`nana-ui-input`、`nana-ui-view-macros`/`nana-ui-view-schema`、`nana-diagnostics`。**不依赖** `nana-ui-css`(运行时不跑 CSS 引擎)与任何 GPU crate。向上,`nana-ui-scene` 依赖 runtime 并在其上做抽取,`nana-ui` 再把 `UiScene` 交给 `SceneWgpuPainter`。相邻关系:

| 邻居 | 关系 |
| --- | --- |
| `nana-ui-core` | 树上样式/主题/几何类型(`NodeStyle` 内含的 `LayoutStyle`、`CompiledTheme` 等)来自 core |
| `nana-text` | 文本节点的测量与保留布局权威,`TextLayoutStore` 挂在 `NodeStore` 里 |
| `nana-ui-scene` | `RuntimeDocument` 用 runtime 的 `AppContext` 驱动整帧(`crates/nana-ui-scene/src/runtime_document.rs:18`) |
| `nana-ui-vue` | 通过 `MutationQueue` 写同一棵树,帧边界提交 `PendingHostOps` |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `UiWorld` | `crates/nana-ui-runtime/src/world.rs:628` | 保留树权威:节点表 + 脏标记 + 各类索引与缓存 |
| `StableNodeId` | `crates/nana-ui-runtime/src/world.rs:70` | 对外稳定节点身份,`u64` 包装,0 保留(缺失/默认不可误寻址) |
| `NodeStore` | `crates/nana-ui-runtime/src/store.rs:379` | 稠密节点表 + 稀疏侧表,替代 `bevy_ecs::World`(文件头注释) |
| `NodeRecord` | `crates/nana-ui-runtime/src/store.rs:81` | 单节点稠密记录:hierarchy、style、resolved、layout、interaction、dirty 等 |
| `Hierarchy` | `crates/nana-ui-runtime/src/store.rs:46` | `parent` / `children: Arc<Vec<StableNodeId>>` / `index_in_parent` |
| `NodeKind` | `crates/nana-ui-runtime/src/world.rs`(经 lib.rs 再导出) | Document / Element / Text / Comment 四类节点 |
| `UiMutation` | `crates/nana-ui-runtime/src/mutation.rs:15` | 一条保留树变异,枚举到 `:302` 为止,整批校验后才改权威 |
| `MutationQueue` | `crates/nana-ui-runtime/src/mutation.rs:306` | 帧局部命令边界,给 reconciler 与适配器用 |
| `CommitReport` | `crates/nana-ui-runtime/src/world.rs:388` | commit 结果计数(created/inserted/detached/reparented/despawned) |
| `ComponentRegistry` | `crates/nana-ui-runtime/src/component_registry.rs:249` | 组件类型注册表:`by_id`/`by_tag`/`by_rust` 三张索引 |
| `DirtyMask` | `crates/nana-ui-runtime/src/schedule.rs:9` | 每节点失效位掩码(`u16`,STYLE…TRANSFORM 共 9 位) |
| `SystemWork` | `crates/nana-ui-runtime/src/schedule.rs:64` | 一次 drain 产出的确定性分系统工作列表 |
| `AppContext` | `crates/nana-ui-runtime/src/framework.rs:1072` | 持有类型化视图状态,`UiWorld` 仍是保留 UI 权威(类型注释) |
| `RuntimeFrameUpdate` | `crates/nana-ui-scene/src/runtime_document.rs:26` | 一帧的结果信封:两个世代 + `SceneDelta` + `AccessibilityDelta` |
| `RuntimeDocument` | `crates/nana-ui-scene/src/runtime_document.rs:18` | 一个文档 + 其增量抽取的 scene,规范宿主的帧事务 |
| `LayoutBoxStore` | `crates/nana-ui-vue/src/tree/layout.rs:39` | Vue 每窗口绘制相位几何,**非布局权威** |

## 内部结构

### 身份与节点存储:稠密表 + 稀疏侧表

`StableNodeId`(`crates/nana-ui-runtime/src/world.rs:70`)是对外的节点身份;`new` 把 0 映射为 `None`,让缺失/默认 ID 无法寻址到活节点(`world.rs:74`)。配套的 `IdHasher`(`crates/nana-ui-runtime/src/world.rs:95`)是「键已是整数」专用哈希:文档注释解释了为什么不用默认 SipHash——采样剖析发现一次变换动画里 SipHash 占了 painter 十分之一时间,而 ID 只是稠密计数器,只需扩散不需防对抗(`world.rs:83`)。

`store.rs` 文件头一句话定调:"Dense node table plus sparse side maps. Replaces `bevy_ecs::World` inside `UiWorld`. Public identity stays `StableNodeId`"(`crates/nana-ui-runtime/src/store.rs:1`)。分两层看:

- **稠密表 `NodeTable`**(`crates/nana-ui-runtime/src/store.rs:300`):`dense: Vec<Option<NodeRecord>>` + `overflow: HashMap`。ID ≤ `DENSE_LIMIT`(65 536,`store.rs:296`)且距表尾不超过 `DENSE_GAP`(1024,`store.rs:297`)时走稠密槽位,否则落入溢出哈希表;`get`/`remove` 都先查稠密再查溢出(`store.rs:307`)。这样常见树是 O(1) 数组寻址,大 ID 也不炸内存。
- **稀疏侧表**(`NodeStore` 的其余字段,`crates/nana-ui-runtime/src/store.rs:379`–445):`visuals`、`custom_render`、`event_listeners`、`component_types`、`overlay_hosts`、`editors`、文本相关一族(`text_fold_views`、`text_inlays`、`text_snippets`…)以及每节点一条的 `text_nodes` 与保留文本布局 `text_layouts`。设计理由写在字段注释里:比如 `text_nodes` 单独成表,是因为大范围重排时要对每个候选快速判定「无文本工作」,小表扫描留在缓存里(`store.rs:436`)。

`NodeRecord`(`store.rs:81`)是最常读的稠密字段集:`hierarchy`、`mount: MountState`、`style: NodeStyle`(按作者原样保留,供投影 diff)、`resolved_layout: Arc<LayoutStyle>`。`resolved_layout` 的文档注释解释了「写入时解析」策略:`LayoutStyle` 有 4.8 KB,若在抽取时解析,1k 个控件切一次调色板要多花三倍代价;改为写样式与装主题时解析一次,读者只克隆 `Arc`(`store.rs:87`)。`Hierarchy` 的 `index_in_parent` 让找兄弟不必扫表——每次编辑子列表时顺带重编号被移动的兄弟(`store.rs:50`)。

### 变异提交:UiMutation / MutationQueue / commit

`UiMutation` 是一个大枚举(`crates/nana-ui-runtime/src/mutation.rs:15`–302),结构变异与状态写都在里面,几类代表:

- 树结构:`Create` / `Insert`(reparent 兼用,`before = None` 即追加)/ `Detach`(脱离但保留、仍 `Mounted`)/ `ParkSubtree`(整棵子树停用、身份保留)/ `DespawnSubtree`(销毁,稳定 ID 不可复用)(`mutation.rs:16`–40)。
- 样式与主题:`SetStyle`、`SetPresetTheme`、`SetThemeTokens`(整体携带 `Arc<CompiledTheme>`,注释明说:两个零散字段从来不是主题的全部,只能表达两类的变异会被后来每个类别绕开,`mutation.rs:48`)。
- 文本族:`SetText`、`SetRichText`、`SetLanguage`、`SetTextScale`、`SetLocale`、`SetLocalizedText`。
- 布局与滚动:`WriteLayout`(注释:引擎写回与测试用,产品 Vue 帧不得用它对抗 `RuntimeLayoutEngine`,`mutation.rs:112`)、`PatchPlacement`、`SetScrollOffset`。

`MutationQueue`(`mutation.rs:306`)只是 `mutations: Vec<UiMutation>` 加一个 `writes_text` 快速标记(让 undo journal 在 O(1) 内跳过无关批,`mutation.rs:308`);它的类型注释称自己是 "Frame-local command boundary for reconcilers and adapters"。队列上的便捷方法(`create`/`insert`/`detach`/`park_subtree`/`despawn_subtree`/`set_style`/`set_rich_text`…,`mutation.rs:313` 起)只做入队,真正的应用在 `UiWorld::commit`(`crates/nana-ui-runtime/src/world/mutation.rs:2915`):整批校验通过才落权威,返回 `CommitReport`(`crates/nana-ui-runtime/src/world.rs:388`)。校验失败对应 `UiWorldError` 的变体:`DuplicateNode`、`RetiredNode`、`MissingNode`、`CrossDocument`、`Cycle`、`TreeTooDeep` 等(`world.rs:414` 起;`TreeTooDeep` 的注释说明深度上限在写树处强制,因为样式/布局/命中/绘制都递归走树,`world.rs:438`)。带挂载生命周期的提交走 `commit_with_mount_lifecycle`,从最终拓扑推导 inserted/parked 集合,避免从 parked 祖先抽取出的后代继承挂起记账(`world/mutation.rs:2922`)。

### 世代与失效记账

`UiWorld` 用三个世代号把「什么变了」限定到最小消费者:

- `generation`:任何权威内容变化都递增,`CommitReport` 返回它,宿主以此判断「这帧有没有事」(`crates/nana-ui-runtime/src/world.rs:628` 起)。
- `layout_generation`(`world.rs:731`):与 `generation` 分开,纯绘制类工作(如合成器 tick)不递增它,布局消费者不必被 paint-only 变化唤醒;`RuntimeFrameUpdate` 同时携带两个世代,帧外观察者可以只订阅布局世代(`crates/nana-ui-scene/src/runtime_document.rs:26`)。
- `layout_source_epoch`(`world.rs:735`):布局**输入**变化才递增;引擎写回走 `WriteLayout` 时不动它(`crates/nana-ui-runtime/src/world/mutation.rs:2704`),防止「写回结果」被误判成「输入变了」而触发整轮重布局。

配套的缓存同样按权威/投影分层:主题以 `Arc<CompiledTheme>` 存放,Style Model 热切片缓存在 `style_model: StyleModelRef`(`world.rs:720`);布局输出权威是 `layout_results: NodeMap<Arc<LayoutResult>>`(`world.rs:741`,见[布局引擎](/learn/runtime-layout)),`NodeRecord` 上的 `LayoutBox` 只是兼容投影。多文档记账也在世界层:`document_viewports`(`world.rs:864`)、`detached`/`detached_mounted`/`live_document_roots`、`nodes_by_component: HashMap<ComponentTypeId, HashSet<StableNodeId>>`(`world.rs:894`,按组件类型反查节点的索引)。

### 组件与 ComponentRegistry

内置组件与扩展组件走同一张注册表。`ComponentRegistry`(`crates/nana-ui-runtime/src/component_registry.rs:249`)维护三张索引:`by_id: HashMap<ComponentTypeId, RegisteredComponentType>`、`by_tag`、`by_rust`(`component_registry.rs:250`–252)。注册入口是 `RegisterableComponent` trait(`component_registry.rs:212`):组件给出 `TYPE_ID`/`TAGS` 常量与 `from_semantic(&SemanticSpec)` 构造,可选 `reconcile_semantic`/`project_semantic`/`finish_semantic`;`RETAIN_SEMANTIC_STATE` 决定交互是否走类型化组件状态而非只写 UiWorld 字段(`component_registry.rs:218`)。语义绑定结果包成 `PreparedSemanticBinding`(`component_registry.rs:155`),其 `kind: ComponentBindKind` 区分 `Projected`(真控件,跳过 Vue 通用样式)与 `Layout`(布局原语,保留 Vue 通用样式/无障碍回写)(`component_registry.rs:178`–184)。`READS_CHILD_DERIVED_SPEC` 是个诚实的性能开关:组件声明自己不读宿主扫描子控件才能推导的 `slots`/`icon` 字段,宿主即可整体跳过那些扫描;注释警告「在这里撒谎的组件会悄悄丢掉槽位子节点和图标」(`component_registry.rs:221`)。AGENTS.md 的「共享 `ComponentRegistry`」合同即落在这一处实现上。

### 两个「非权威」投影:RuntimeDocument、LayoutBoxStore 与 PendingHostOps

一条硬纪律:布局与绘制侧的一切表都是投影,不构成第二权威。三个实例:

- **`RuntimeDocument`**(`crates/nana-ui-scene/src/runtime_document.rs:18`):一个 `AppContext` + `DocumentId` + `Arc<UiScene>` 的组合,规范宿主用它保证「文本、布局、命中测试、无障碍、渲染抽取在同一有界帧事务内排干」(类型文档,`runtime_document.rs:14`)。
- **`LayoutBoxStore`**(`crates/nana-ui-vue/src/tree/layout.rs:39`):Vue 每窗口的绘制相位几何,字段注释直说 "Not layout authority"——它记录 Scene 绘制回写的盒子,「滚动活在 JS overlay 里,不写回 Runtime `LayoutBox`」(`tree/layout.rs:34`)。它的 `revision: AtomicU64` 只在**有效变化**时递增:Scene 帧每帧都重录可见节点,「被写过」每帧为真、毫无信号价值,读者要的是「和上次不同」(`tree/layout.rs:43`)。
- **`PendingHostOps`**(`crates/nana-ui-vue/src/tree.rs:221`):Vue 外观(facade)写的帧局部暂存——`mutations: MutationQueue` 加 parent/children/kinds/texts/events/gpu 六张读穿镜像表;`mutations` 是否为空就是是否需要 commit 的信号,`commit_pending_queue` 两侧一起清、永不单清(`tree.rs:235` 注释)。宿主帧边界在 `flush_host_frame` 里先 `commit_pending_queue` 再 `flush_runtime_systems`(`tree.rs:753`)。这正是 AGENTS.md「`PendingHostOps` 在帧边界 commit」合同的实现位置——注意它属于 `nana-ui-vue`,不是 runtime 本体。

### 运行时系统:DirtyMask、SystemWork 与 flush 顺序

变异落库时,commit 在每个受影响节点上打位;帧驱动再把位图排成确定性工作:

1. **失效位**:`DirtyMask(u16)`(`crates/nana-ui-runtime/src/schedule.rs:9`)九位:`STYLE`/`TEXT`/`LAYOUT`/`INPUT`/`FOCUS_IME`/`RENDER`/`ACCESSIBILITY`/`STATE`/`TRANSFORM`(`schedule.rs:12`–26)。注释强调合同粒度:PAINT 不是独立位(纯绘制变异只置 RENDER 不置 LAYOUT);STATE/TRANSFORM 独立成位,让 hover/变换失效不被算成样式或布局(`schedule.rs:17`)。
2. **drain**:`UiWorld::take_system_work`(`crates/nana-ui-runtime/src/world.rs:2286`)把 `dirty_entities` 排序后逐个取位,分拣进 `SystemWork`(`schedule.rs:64`)的 style/state/text/layout_frontier_seeds/input_hit_test/focus_ime/accessibility/render_extraction 各列表。布局负载只从类型化侧表 `pending_layout_invalidations` 取,私有 LAYOUT 位只负责让 drain 访问该实体(`world.rs:2343` 注释)。`AppContext::take_system_work`(`crates/nana-ui-runtime/src/framework/frame.rs:13`)在这之前还会 `poll_tasks()` 并 `flush_reactive()`——注释提醒持有 `AppContext` 的调用者用它而非直接 drain `compat_world_mut()`,否则绕过这两步。
3. **规范帧循环**:`RuntimeDocument::flush`(`crates/nana-ui-scene/src/runtime_document.rs:109`)是权威顺序的现场:每轮 `take_system_work` → `resolve_styles(&work.style)` → `shape_text` → 合并文本整形新发现的类型化种子 → `layout_document_for_viewport` / `layout_document_with_frontier` → 对重排范围做最多 `POST_LAYOUT_SHAPE_PASSES = 4`(`runtime_document.rs:48`)轮「重整形→再布局」反馈;整个循环以 `MAX_FRAME_PASSES = 8`(`runtime_document.rs:11`)封顶,不收敛报 `FrameworkError::FrameDidNotSettle`。循环 settle 后才一次性做:命中测试工作(注释:文本/布局反馈可能跨轮重复弄脏同一批节点,逐轮重建只为最后一次可见的结果买单 N 次,`runtime_document.rs:222`)→ `project_accessibility_delta` → `extract_nodes` + `scene.apply_delta`(把 delta 应用进保留 scene 也计入 Extract 阶段计时,`runtime_document.rs:285`)→ 合成器 `apply_presentation`。对应 `AppContext` 的分阶段入口在 `framework/frame.rs`:`resolve_styles`(:87)、`resolve_presentations`(:105)、`shape_text`(:112)、`shape_text_for_layout`(:130)、`layout_document`(:170,全量)、`layout_document_with_frontier`(:182,增量)、`layout_document_for_viewport`(:196,视口变化)。

抽取侧有两个入口:`UiWorld::extract_nodes` 只抽脏节点,隐藏节点保留在结果里且 `visible=false`,让增量渲染器能移除其旧图元(`crates/nana-ui-runtime/src/world/extraction.rs:652`);`extract_document` 按保留文档序出全量快照(`extraction.rs:665`)。

## 协作与数据流

一条完整链(规范 Rust 宿主):

```text
应用/组件代码 ──写──▶ MutationQueue(帧局部)
                        │ UiWorld::commit(整批校验+应用)  world/mutation.rs:2915
                        ▼
                UiWorld(NodeStore + DirtyMask 位)
                        │ RuntimeDocument::flush → AppContext::take_system_work
                        ▼
        SystemWork{style,text,layout seeds,input,a11y,render}
                        │ resolve_styles → shape_text → layout_document_*
                        ▼
        extract_nodes → UiScene.apply_delta(scene 侧,详见场景抽取章)
```

- 与布局:commit 产生的类型化失效种子 `LayoutFrontierSeed` 交给增量布局,见[布局引擎](/learn/runtime-layout)。
- 与输入:`route_input` 只消费已提交的树与命中索引,见[输入链路](/learn/runtime-input)。
- 与工作台:Shell/Workspace/Dock 都是**树上的组件**,通过同一 `MutationQueue` 写状态,不另设存储,见[工作台层](/learn/workspace-shell-dock)。
- 与 Scene:runtime 只产出 `ExtractedNode` 增量,保留 scene 的结构与合成器动画归 `nana-ui-scene`,参见场景抽取章(gpu-frame 章讲绘制)。
- 世代纪律:`generation`/`layout_generation`/`layout_source_epoch` 三号分离,让 paint-only 帧、布局输入变化、权威变化各有独立的失效通道(见上文「世代与失效记账」)。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-runtime/src/world.rs` | `UiWorld` 定义、`StableNodeId`/`IdHasher`、`CommitReport`、`UiWorldError`、`take_system_work` |
| `crates/nana-ui-runtime/src/store.rs` | `NodeStore`/`NodeRecord`/`Hierarchy`/`NodeTable`(稠密+溢出)与全部稀疏侧表 |
| `crates/nana-ui-runtime/src/mutation.rs` | `UiMutation` 枚举与 `MutationQueue` 及其入队方法 |
| `crates/nana-ui-runtime/src/world/mutation.rs` | `UiWorld::commit`/`commit_with_mount_lifecycle`,变异应用与滚动记账 |
| `crates/nana-ui-runtime/src/component_registry.rs` | `ComponentRegistry`、`RegisterableComponent`、`PreparedSemanticBinding` |
| `crates/nana-ui-runtime/src/schedule.rs` | `DirtyMask`、`SystemWork`、`push_work` |
| `crates/nana-ui-runtime/src/framework.rs` | `AppContext`(类型化视图状态层) |
| `crates/nana-ui-runtime/src/framework/frame.rs` | 帧操作:drain、分阶段 resolve/shape/layout 入口 |
| `crates/nana-ui-runtime/src/world/extraction.rs` | `extract_nodes`/`extract_document` |
| `crates/nana-ui-scene/src/runtime_document.rs` | `RuntimeDocument::flush`,规范帧事务顺序 |
| `crates/nana-ui-vue/src/tree.rs` | `PendingHostOps`、`flush_host_frame`(Vue 帧边界) |
| `crates/nana-ui-vue/src/tree/layout.rs` | `LayoutBoxStore`(非权威的绘制相位几何) |

## 延伸阅读

- [布局引擎](/learn/runtime-layout):本章的失效种子在布局侧怎么被消费。
- [输入链路](/learn/runtime-input):`SystemWork.input_hit_test` 之后的路由。
- [合同与不变量](/learn/contracts-and-invariants):「一棵权威树」等非协商合同的完整清单。
- [怎么读这份代码](/learn/reading-the-code):按入口文件找更多样例。
- 参考层的[架构总览](/reference/architecture)与[工作原理](/reference/how-it-works)。
