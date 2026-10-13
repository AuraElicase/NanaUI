---
title: 布局引擎
---

# 布局引擎内部:RuntimeLayoutEngine

这一章拆开 `nana-ui-runtime` 的布局引擎:样式怎么进入测量、容器怎么选择格式化上下文、增量布局靠什么决定「只重排该重排的子树」、文本测量与布局如何互相反馈、滚动为什么不写布局盒。读完你能搞懂:`layout_document_with_frontier` 一次增量遍的完整路径;`LayoutFrontier` 依赖图与各类失效种子的关系;`IntrinsicMetrics`/`LayoutResult` 两个输出合同的区别;以及视口 resize、文本反馈、滚动各自触发的失效范围。本章承接[运行时保留树](/learn/runtime-tree)的 commit/flush 模型,是四个运行时章里最厚的一块。

## 职责与边界

- **权威算法,无状态外壳**:`RuntimeLayoutEngine` 是个零字段单元结构(`#[derive(Default, Clone, Copy)]`,`crates/nana-ui-runtime/src/layout_engine.rs:88`–89),类型文档定位它是 "Backend-neutral layout owner used by canonical Runtime applications":消费 `UiWorld` 里已存的 `LayoutStyle` 与整形后的文本度量,返回原子布局写回。跨帧状态全部放在调用方持有的 `RetainedLayoutCache`(`layout_engine.rs:719`),引擎本体不持有。
- **输入边界**:运行时不解析 CSS。进入引擎的样式是 Style Model 数据(`nana_ui_core::LayoutStyle`,由 `NodeStyle` 携带、样式解析后挂在 `NodeRecord.resolved_layout` 上);`nana-ui-css` 引擎负责把 CSS 映射成 Style Model(见 [CSS 引擎](/learn/css-engine))。
- **输出边界**:输出的权威形态是 `LayoutResult`(`crates/nana-ui-runtime/src/layout_result.rs:113`);`LayoutBox` 只是旧调用方的兼容投影——模块头注释(`layout_result.rs:1`–7,Issue #203)写明新消费者应保留 `LayoutResult`,让 Scene、命中测试、无障碍、滚动无法从组件状态里各自推导出不同的几何。
- **不做的事**:不画图(布局片段类型 `LayoutFragmentKind` 刻意只描述布局输出,不含渲染图元,`layout_result.rs:17`);不直接改树——写回也要走 `MutationQueue::write_layout`(见「写回」小节)。

## 依赖与邻居

| 邻居 | 关系 |
| --- | --- |
| `nana_ui_core` | 布局合同:`LayoutStyle`、`LengthSpec`、`FlexDirection`/`GridTrack`/`GridPlacement` 等规范类型,以及失效分类 `LayoutInvalidation`/`InvalidationKind`/`LayoutDependencyFootprint`、Dynamic Layout 的 `solve_deficit`/`EnvelopeShape` |
| `nana-text` | 文本叶子测量与保留文本布局权威;引擎通过 `UiWorld` 侧表读取整形结果 |
| `UiWorld` | 输入树 + 写回目标;增量种子由 commit 产生(`pending_layout_invalidations`),见[运行时保留树](/learn/runtime-tree) |
| `AppContext` 帧驱动 | `framework/frame.rs` 的 `layout_document*` 系列入口把引擎接进帧循环 |
| css-parity / Vue `measure_layout` | 通过 `layout_style_tree` 复用同一算法,不养第二套布局(`layout_engine.rs:85`–87) |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `RuntimeLayoutEngine` | `crates/nana-ui-runtime/src/layout_engine.rs:89` | 布局算法所有者,无状态 |
| `LayoutViewport` | `crates/nana-ui-runtime/src/layout_engine.rs:50` | 平台宿主给的逻辑视口(`width`/`height`,构造即钳为有限值) |
| `LayoutInputMap` | `crates/nana-ui-runtime/src/layout_engine.rs`(`mod context`) | 一遍布局对 `UiWorld` 的预取视图(样式、流子列表、文本) |
| `EstablishedContext` | `crates/nana-ui-runtime/src/layout_engine/context.rs:13` | 容器为其流入子代建立的格式化上下文 + `grid_2d` 标志 |
| `IntrinsicMetrics` | `crates/nana-ui-runtime/src/intrinsic_cache.rs:57` | 子树固有贡献:min/max inline·block、preferred、基线、宽高比、`truncatable`、`generation` |
| `UsedSize` | `crates/nana-ui-runtime/src/intrinsic_cache.rs:21` | 某个包含块解析出的已用尺寸(inline/block 逻辑轴) |
| `LayoutFrontierSeed` | `crates/nana-ui-runtime/src/layout_frontier.rs:19` | 种子 = 节点 + `LayoutInvalidation`(失效类/字段掩码/依赖足迹) |
| `LayoutDependencyGraph` | `crates/nana-ui-runtime/src/layout_frontier.rs:61` | 保留依赖索引:parents/children/contexts 三张邻接表 + 隔离集 |
| `LayoutFrontier` | `crates/nana-ui-runtime/src/layout_frontier.rs:318` | 一遍的 measure/placement/context/writing/scroll 工作并集 |
| `RetainedLayoutCache` | `crates/nana-ui-runtime/src/layout_engine.rs:719` | 跨帧布局记忆:每文档的盒子、内蕴事实、容器/测量计划 |
| `LayoutResult` | `crates/nana-ui-runtime/src/layout_result.rs:113` | 单节点规范几何快照(border/padding/content box、fragments、parts、clip、依赖) |
| `LayoutResultSource` | `crates/nana-ui-runtime/src/layout_result.rs:105` | `RuntimeLayout` 与 `CompatibilityWrite` 两种来源戳 |
| `TextLayoutKey` / `TextLayoutCache` | `crates/nana-ui-runtime/src/text_layout_cache.rs:17` / `:89` | 有界 FIFO 文本度量缓存(容量 2048,`:14`),键含字体代 |
| `Extent` / `MaxTree` | `crates/nana-ui-runtime/src/world/scroll_bounds.rs:13` / `:60` | 滚动内容范围的区间并 + 线段树聚合 |

## 内部结构

### 输入与组织:从 Style Model 到格式化上下文

引擎读的不是原始 `NodeStyle`,而是解析链的产物。`UiWorld::resolve_style`(`crates/nana-ui-runtime/src/world/style.rs:87`)自根向下解析,每节点产出 `ResolvedStyle(Arc<ComputedStyle>, palette_epoch)`;布局读的版本是 `motion_layout(id, &resolved_layout)`——「设计意图加上响应式变体后的布局;文本、绘制与可见性跟随变体」(`world/style.rs:106`–108)。两个关键捷径:

- **继承共享**:无本地覆盖的节点直接共享父节点的 `Arc<ComputedStyle>`(`publish_inherited_computed_style`,`world/style.rs:12`)。构造一个 `ComputedStyle` 要读 4.8 KB 的布局样式才发现答案就在父节点上,共享 `Arc` 与完整解析器殊途同归(`world/style.rs:8`–11)。
- **写时解析**:`NodeRecord.resolved_layout: Arc<LayoutStyle>` 在写样式/装主题时解析一次(理由见[运行时保留树](/learn/runtime-tree)的存储节)。

百分比边距的解析基准是包含块的 inline 尺寸(`measure.rs` 的 `edge_base` 参数注释指向 `UiWorld::containing_writing`;`inline.rs:20`–22 引 CSS Box Model §5)。`resolved_size_specs`(`crates/nana-ui-runtime/src/layout_engine/measure.rs:33`)从样式单独解析节点自身宽高:`Some` 表示该轴已用尺寸与子代无关,内容尺寸关键字(`min-content`/`max-content`/`fit-content`/`shrink`)与不定 `Fill` 都归 `None`,交给内蕴测量。

上下文选择集中在 `establish_context`(`crates/nana-ui-runtime/src/layout_engine/context.rs:29`):`display` 是 grid 容器 → `FormattingContextKind::Grid`;flex 容器 → `Flex`;否则流入子里出现 inline 级子代 → `Inline`;否则 `Flow`。模块头注释强调「测量与放置都问这一个函数,两遍不会对同一节点选不同算法」(`context.rs:1`–6)。`grid_2d` 区分「有显式轨道/areas/放置项的 grid 走二维轨道求解器,没有的退化为单隐式列、由行打包器等价放置」(`context.rs:14`–18)。

各上下文的算法分文件:`flow.rs` 收集流入子列表,`flex.rs` 的 `distribute_flex_main`(`crates/nana-ui-runtime/src/layout_engine/flex.rs:25`)做主轴分配,`grid.rs` 的 `Grid2DLayout`(`crates/nana-ui-runtime/src/layout_engine/grid.rs:23`)解轨道(`InheritedGridTracks` 支持子网格继承已解析轨道,`grid.rs:16`),`inline.rs` 的 `pack_wrap_lines`(`crates/nana-ui-runtime/src/layout_engine/inline.rs:6`)做换行装箱。两个值得记住的细节:

- `collect_flow_children_reporting` 返回 `(流入子列表, 是否依赖更深层后代)`(`crates/nana-ui-runtime/src/layout_engine/flow.rs:23`)。两个会打破「流入列表是直接子列表的子序列」的情形都如实上报:`display: contents` 把子的子拼进流入列表;块父之下的 inline 级子代是否拆箱取决于其子树深处有没有块(`flow.rs:5`–22)。上报条件而不是事后检查,把这事从每兄弟路径上挪走。
- **Dynamic Layout**(`crates/nana-ui-runtime/src/layout_engine/dynamic.rs:1`–18,Issues #209/#212/#213):设置了 `AdaptationProfile::solve_overflow` 的容器在单行放不下时,先问自动尺寸的子代「能让出什么」(`DynamicPass::envelope`),按最便宜顺序取(`solve_deficit`);没在解算的容器永不读子代声明,声明弹性在无人问津时零开销。包络与行解算都跨遍保留(`RetainedEnvelope` `:41`、`RetainedLineSolve` `:50`):包络只依赖样式与子代包络、不依赖被提供的尺寸,所以 resize 后全部命中;行解算保留停在哪一级(`SolveFrontier`),赤算与增量收敛到同一答案。

### 两遍结构与跨帧缓存:intrinsic → place,计划复用

每遍布局是经典两阶段:`intrinsic_size`(测量,`crates/nana-ui-runtime/src/layout_engine/measure.rs:6`)自底向上回答「子树要多少」,`place_node`(放置,`crates/nana-ui-runtime/src/layout_engine/placement.rs:6`)自顶向下把盒子放进包含块。增量版是 `place_node_scoped` + `ScopeContext`(affected/measure/reach 视图,`layout_engine.rs:350`–356)。

跨帧记忆分三层,全在 `RetainedLayoutCache`(每文档一份,一个窗口的全量遍不会作废另一个窗口的布局,`layout_engine.rs:715`–721):

1. **内蕴事实缓存**(generation-aware,帧内 + 保留事实):模块头(`intrinsic_cache.rs:1`–13)讲清了键的设计——内蕴度量描述内容子树,与「解析某个包含块得到的 `UsedSize`」刻意分开;键只含内容/样式身份与**约束的类别**,不含格式化上下文身份,flex/grid/block/inline 问同一个问题就命中同一条。
2. **容器计划 `ContainerPlan` 与测量计划 `MeasurePlan`**:放置重放缓存的容器计划,`PlanCheck` 三态(`placement.rs:33`–43):`Unchanged` 全保;`ChangedFrom(idx)` 从最低变化条目起重放;`Stale` 全量。计划发布有「Gate E:一容器一计划,重复编辑替换而非追加」(`layout_engine.rs:454`–458);测量计划按 (容器, 约束) 槽位**合并而非替换**——容器常在一遍里被两种约束测量但只重测其一,替换会让两槽位交替、稳不住(`layout_engine.rs:480`–491)。本遍没量到的约束槽还要主动查 `plan_missed_change`(定义在 `measure.rs:1685`),否则计划会吐出变化前的子代尺寸(注释与调用见 `layout_engine.rs:493`–509)。
3. **全量快照复用**:`force_full` 且布局源没变(`layout_source_epoch` 相同)且视口相同时,直接整块克隆上次的输出,连测量/放置都不跑(`layout_engine.rs:214`–246)。

### 增量与失效:LayoutFrontier 与依赖图

增量入口 `layout_document_with_frontier`(`layout_engine.rs:150`)接收类型化种子。种子的传播不是「脏祖先链」,而是一张按足迹加权的依赖图:

- `LayoutDependencyGraph`(`layout_frontier.rs:61`)只存局部邻接:父子边双向(子代度量上行走 parent 链,父约束下行走 child 链,`add_parent_dependency_split` 用两份独立的 `LayoutDependencyFootprint` 防止 placement-only 种子把两个方向都撑满,`layout_frontier.rs:103`–116)、上下文双向边(flex 行/grid 轨道这类兄弟域,`:120`)、隔离集(`:140`)。邻接表用 `by_target` 索引去重合并,「扫列表查重会把全带边的父代做成子代数平方」(`layout_frontier.rs:34`–36`)。
- `FrontierEntry` 把失效拆成 measure/placement/context/writing/scroll/topology 六个工作位(`layout_frontier.rs:225`–252);`merge` 只在**工作类或依赖足迹**扩大时宣布「需要再传播」——reason/source 这类诊断元数据变了不触发又一次祖先遍历(`layout_frontier.rs:254`–290)。
- 传播只沿「足迹消费了该度量」的边走(`layout_engine.rs:148`–149 注释)。输出的变化分类由 `LayoutResult::metric_delta`(`layout_result.rs:205`)给出:`INTRINSIC_INLINE/BLOCK`、`BASELINE`、`USED_SIZE`、`PLACEMENT`——只动原点不会误判成 USED_SIZE 让父代白测一遍(`layout_result.rs:219`–229)。

视口变化是增量的一等公民,靠两个机制保住无关盒子的缓存:

- **按读取裁剪的视口基**:每个盒子的 memo/计划比较的是 `viewport_basis`——只含它实际解析时会读的边(`vw` 读宽、`vh` 读高、`position: fixed` 都读),不读的边记 0(`layout_engine.rs:64`–77)。于是视口 resize 保留所有不读移动边的盒子;尺寸跟着「会读的后代」动的盒子自然在 resize 种子的 frontier 上。
- **resize 种子**:`UiWorld::viewport_resize_seeds`(`crates/nana-ui-runtime/src/world.rs:3800`)只种两类:每个根(约束在移动轴上的消费者)与「解析时读了移动边」的盒子;`vw` 盒子不会被纯高度 resize 种到(`world.rs:3793`–3799)。

### 与文本测量的交互

文本叶子由 `nana-text` 测量,引擎经 `TextLayoutCache` 复用度量:键 `TextLayoutKey`(`text_layout_cache.rs:17`)覆盖文本、字号/字重/字族、行高、字距、特性/变体、断行与换行策略、wrap/ellipsis/max_lines,以及约束(`max_width_bits`/`max_height_bits`)和**宿主字体代**——「对着已换掉的字体集测出的度量必须 miss 而不是替新字体回答」(`text_layout_cache.rs:41`–43)。缓存是有界 FIFO(默认 2048 条,`:14`),查/插是 `text_layout_cache_*` 计数的唯一命中/未命中来源,模块头特别声明它不是字形图集(`text_layout_cache.rs:1`–5)。

布局与整形是**反馈环**:布局定了行宽 → 重新整形 → 文本度量变 → 可能又改布局。规范帧里这个环被 `POST_LAYOUT_SHAPE_PASSES = 4`(`crates/nana-ui-scene/src/runtime_document.rs:48`)封顶:scoped 重整形后若没有新种子就停,否则再排一遍,「永不收敛的布局不能霸占这一帧」。`AppContext` 侧的对应入口是 `shape_text_for_layout` / `shape_text_for_layout_scoped`(`crates/nana-ui-runtime/src/framework/frame.rs:130`/`149`)。另一条纪律:`layout_style_tree` 让宿主传入**它帧里用的同一个 shaper**——「用第二个 shaper 测出的样式树,给出的盒子画出的文本塞不下」(`layout_engine.rs:618`–620)。

### 滚动:范围是布局的,偏移不是

滚动模型一句话(模块头,`world/scroll_bounds.rs:1`–4):**内容范围是纯布局产物;滚动偏移与绘制效果不改变可滚动布局范围**;范围在最邻近的裁剪盒处截断,与 CSS scrollable overflow 一致。

- **范围聚合**:`Extent`(`scroll_bounds.rs:13`)是滚动容器坐标下「到达滚动区域的子孙盒子之并」,空集取反(`left > right`)以便作为合并幺元;`MaxTree` 线段树 O(n) 原地重建,叶数不变时复用缓冲(`scroll_bounds.rs:65`–89)。
- **写偏移不写盒子**:`scroll_to_clamped`(`crates/nana-ui-runtime/src/world/mutation.rs:2758`)只改 `NodeRecord.scroll_offset`,然后打 `scroll_hit_updates`(命中索引增量)并标 `DirtyMask::INPUT | DirtyMask::RENDER`——注释原话:「子孙保留 LayoutBox;绘制用 scroll_offset」(`world/mutation.rs:2763`–2764)。这正是「滚动不写 Runtime 布局盒」合同的落点。`LayoutResult.scroll_extent` 的字段注释同样声明:纯绘制的滚动变化保持结果 generation 不变,范围变了才生成新结果(`layout_result.rs:124`–127)。
- **API 面**:`AppContext::scroll_to`(`crates/nana-ui-runtime/src/framework/scroll.rs:157`)钳制后走 `set_scroll_offset` 变异 + commit,再发 `ScrollChanged`;`scroll_into_view`(`:206`)用上遍布局发布的盒子、移动最小距离,且不物化虚拟行;焦点落位由 `reveal_focused_target`(`:222`)逐祖先滚动口揭示。
- **commit 侧收尾**:每次 commit 后 `remeasure_scroll_containers`(`world/mutation.rs:2792`)对内容被本批移动的滚动容器重测范围(靠内容索引判脏,每个容器一次检查而非每个盒子一次遍);布局遍之后 `apply_document_scroll_retention`(`crates/nana-ui-runtime/src/framework/frame.rs:494`)补发 `ScrollChanged`/`ScrollLaidOut`/`ScrollViewportChanged` 并做 follow-end/锚点保持。

### 写回与发布

引擎不直接改树。`layout_document_observed`(`crates/nana-ui-runtime/src/framework/frame.rs:345`)拿到输出后:对每个盒子,只有 padding、far-start(滚动原点)或盒子本身变化才入队 `mutations.write_layout(id, layout)`(`frame.rs:400`–420);`commit_mutations` 之后**等 scope 内所有兄弟盒都提交完**,才 `publish_layout_results(&scope, LayoutResultSource::RuntimeLayout)` 发布一份一致的 `LayoutResult` 快照——commit 期间的过渡 `LayoutBox` 写会被这里的 RuntimeLayout 来源戳替换(`frame.rs:444`–449`)。`publish_layout_results`(`crates/nana-ui-runtime/src/world.rs:1459`)把祖先闭包一并重发布,免得兼容写留下指向旧子盒的父结果;几何未变的节点直接复用旧结果。对外读取入口是 `UiWorld::layout_box`(`world.rs:2629`),它投影的就是 `LayoutResult` 的权威 border box。失败路径会 `restore_layout_frontier_seeds`(`frame.rs:316`),种子不丢,下一帧重试。顺带一提:commit 内部对 `WriteLayout` 有特殊待遇——它不计入 `layout_inputs_changed`(`world/mutation.rs:2704`–2706),所以引擎写回不会自己抬高 `layout_source_epoch` 又把自己的缓存作废。

## 协作与数据流

```text
commit(SetStyle/Insert/…) ──产出──▶ LayoutFrontierSeed(类型化失效)
RuntimeDocument::flush ──驱动──▶ AppContext::layout_document_with_frontier
    ▶ 种子 + take_layout_frontier_seeds 合并            frame.rs:264-281
    ▶ 视口变了:补 viewport_resize_seeds                  layout_engine.rs:198-213
    ▶ LayoutDependencyGraph 闭包 → LayoutFrontier
    ▶ intrinsic_size(内蕴缓存)/ place_node(计划重放)
    ▶ 仅变化盒子入队 write_layout → commit → publish_layout_results
    ▶ scoped 重整形(≤4 轮)有新种子则再来一遍
输出:LayoutBox(兼容)+ LayoutResult(规范,Scene/命中/a11y/滚动共用)
```

- 与样式:`resolve_styles` 先于布局跑,响应式变体在 `motion_layout` 处合入(见[运行时保留树](/learn/runtime-tree)的 flush 顺序节)。
- 与文本:文本位脏 → `shape_text` → 布局消费度量;布局改行宽 → scoped 重整形反馈环。
- 与命中测试:`LayoutResult` 是命中几何的来源之一;滚动的命中索引增量走 `scroll_hit_updates`(见[输入链路](/learn/runtime-input))。
- 与场景抽取:`extract_nodes` 读 `LayoutResult` 投影;合成器动画不写回基础样式,所以布局结果对纯动画帧稳定。
- 与宿主:Vue 侧的 `LayoutBoxStore` 是绘制相位投影、非权威(见[运行时保留树](/learn/runtime-tree));JS overlay 滚动不回写。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-runtime/src/layout_engine.rs` | 引擎入口(全量/增量/样式树)、`LayoutViewport`、`RetainedLayoutCache` |
| `crates/nana-ui-runtime/src/layout_engine/context.rs` | `establish_context`:格式化上下文选择 |
| `crates/nana-ui-runtime/src/layout_engine/measure.rs` | `intrinsic_size` 与「样式自己能定多少」的判定函数族 |
| `crates/nana-ui-runtime/src/layout_engine/placement.rs` | `place_node`、容器计划重放、`PlanCheck` |
| `crates/nana-ui-runtime/src/layout_engine/flow.rs` / `flex.rs` / `grid.rs` / `inline.rs` | 各上下文的收集/分配/轨道/装箱算法 |
| `crates/nana-ui-runtime/src/layout_engine/dynamic.rs` | Dynamic Layout:弹性声明、包络、行解算 |
| `crates/nana-ui-runtime/src/layout_frontier.rs` | 种子、依赖图、frontier 构建与合并语义 |
| `crates/nana-ui-runtime/src/intrinsic_cache.rs` | `IntrinsicMetrics`/`UsedSize` 与跨上下文共享的内蕴缓存键 |
| `crates/nana-ui-runtime/src/text_layout_cache.rs` | 有界 FIFO 文本度量缓存 |
| `crates/nana-ui-runtime/src/layout_result.rs` | `LayoutResult`/`LayoutFragment`/`LayoutPart`/`metric_delta` |
| `crates/nana-ui-runtime/src/world/style.rs` | 样式解析、继承共享、`motion_layout` |
| `crates/nana-ui-runtime/src/world/scroll_bounds.rs` | `Extent`/`MaxTree` 滚动内容范围 |
| `crates/nana-ui-runtime/src/world/mutation.rs` | commit 侧:`scroll_to_clamped`、滚动容器重测、结果发布 |
| `crates/nana-ui-runtime/src/framework/frame.rs` | 帧驱动布局入口与写回发布 |
| `crates/nana-ui-runtime/src/framework/scroll.rs` | `scroll_to`/`scroll_into_view`/焦点揭示 |

## 延伸阅读

- [运行时保留树](/learn/runtime-tree):种子从哪来(commit),布局完谁来消费(flush)。
- [文本引擎](/learn/text-engine):`nana-text` 侧的测量、整形与保留文本布局权威。
- [CSS 引擎](/learn/css-engine):CSS 如何映射成 Style Model,再进本章的输入。
- [场景抽取与合成](/learn/scene-extraction):`LayoutResult` 之后,盒子怎么变成可绘制节点。
- [输入链路](/learn/runtime-input):命中测试如何使用布局几何。
