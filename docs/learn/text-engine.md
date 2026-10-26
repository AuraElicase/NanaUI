---
title: 文本引擎：nana-text
---

# 文本引擎：`nana-text`

本章深入 `nana-text`：全框架唯一的文本测量、OpenType 整形（shaping）与保留文本布局权威。读完后你能搞懂：一次 `TextEngine::layout` 调用中「字体选择 → 文本整形 → 行排版」三段各自做什么、缓存边界在哪里；字体层如何用世代（`FontGeneration`）与 `cmap` 覆盖率决定「每个字素簇使用哪个字体面（face）」；为什么将行排版与整形彻底解耦能让窗口连续缩放只花折行的代价而不重跑整形；以及 Runtime 如何通过 `TextLayoutStore` 世代句柄跨帧保留文本布局。

## 职责与边界

`crates/nana-text/src/lib.rs`（`:7-21`）清晰界定了文本栈的领地：

- **阶段一：字体层（#90）**：负责字体注册、世代管理、字体面匹配、可变字体轴坐标解析与基于 `cmap` 覆盖率的缺字回退。
- **阶段二：OpenType 整形（#91）**：负责字素簇/脚本/双向文本（BiDi）分段、HarfRust 整形与有界整形缓存（`ShapeCache`）。
- **阶段三：段落行布局（#92）**：负责单行标签快路径、基于 UAX #14 断行机会的贪心折行、逐行 BiDi 视觉重排、行盒度量、对齐、省略号截断与有界布局缓存（`LayoutCache`）。`NativeTextEngine` 将这三段统一收在 `TextEngine::layout` 一个入口后面。
- **阶段四至八：保留句柄与唯一产品权威（#95 / #99）**：`UiWorld` 的文本节点通过 `TextLayoutStore` 世代句柄保留排版结果，Rust L3 与 `.vue` 共用同一台进程级引擎测量，并由 `nana-ui` 的 `NanaRenderer::text` 直接绘制保留的 `TextLayout`。
- **底层算法封装与词汇借用**：`cosmic-text` 与 `cryoglyph` 已从依赖图中彻底移除（由 `scripts/check-engine-boundary.py` 守门）；底层标准库（`fontdb`、`skrifa`、`harfrust`、`unicode-bidi`、`unicode-linebreak`）严格封闭在各自的私有子模块内，绝不泄漏到公开签名中；12 个排版枚举（如 `FontVariationSetting`、`LineBreakSpec`、`WritingModeSpec` 等，`lib.rs:30-37`）直接从 `nana-ui-core` 白名单导入，不重复定义。
- **光标与编辑是 IR 上的纯推导**：光标定位、命中测试与选区矩形是已完成 `TextLayout` 上的纯函数推导（`crates/nana-text/src/edit.rs`），不污染引擎缓存状态。

## 依赖与邻居

`crates/nana-text/Cargo.toml`（`:16-37`）规定了每个外部算法库唯一允许出现的私有文件：

| 依赖库 / 邻居 Crate | 唯一允许引用的模块 / 协作关系 |
| --- | --- |
| `nana-ui-core` | 提供 12 个排版枚举词汇与 `WritingContext` |
| `fontdb 0.24` | 系统字体目录扫描，仅限 `font/discovery.rs` |
| `skrifa 0.44` / `read-fonts` | 可变字体轴、命名实例、彩色字形表与 `cmap`，仅限 `font/face.rs` |
| `icu_properties 2.3` | Unicode Script 与 `Emoji_Presentation` 属性，仅限 `font/unicode.rs` |
| `harfrust 0.12` | OpenType 整形（GSUB / GPOS），仅限 `shaping/opentype.rs` |
| `unicode-bidi 0.3.18` | Unicode 双向文本算法（UBA），仅限 `shaping/bidi.rs` |
| `unicode-linebreak 0.1.5` | UAX #14 断行机会表，仅限 `layout/breaks.rs` |
| `nana-ui-runtime` | 最大消费方：经 `SharedTextEngine` 测量，经 `TextLayoutStore` 保留句柄 |
| `nana-ui` | `NanaRenderer::text` 直接绘制 Runtime 保留的 `TextLayout` |

## 关键类型与数据

| 类型 | 所在文件与行号 | 作用说明 |
| --- | --- | --- |
| `TextEngine` | `crates/nana-text/src/engine.rs:25` | 三段管线总接口：仅包含 `font_generation` 与 `layout` 两个方法 |
| `NativeTextEngine` | `crates/nana-text/src/engine.rs:79` | `FontSystem` + `Shaper` + `Layouter` + 省略号源 + 语言提示 + strut 缓存 |
| `SharedTextEngine` | `crates/nana-text/src/engine.rs:50` | `Arc<Mutex<NativeTextEngine>>`：全进程所有窗口与绘制器共享同一实例 |
| `TextSource` / `TextStyle` / `TextKind` | `crates/nana-text/src/source.rs` / `style.rs:28` / `:16` | 带富文本跨度与内联对象的文本源、基础样式、文本类别（`Label` / `Paragraph` / `Editable`） |
| `TextConstraints` | `crates/nana-text/src/constraints.rs:30` | 容器约束：最大宽高、折行策略、`max_lines`、省略号、书写模式、对齐、Tab 宽度 |
| `ShapedText` / `ShapedRun` | `crates/nana-text/src/shaping/` / `shape.rs` | 整形产物：不可变字形运行序列（携带方向、脚本与直立/侧卧取向） |
| `LayoutRequest` | `crates/nana-text/src/layout/engine.rs:33` | 单次行排版输入：只读的 `ShapedText`、容器约束、省略号、strut 度量、ruby/标签 |
| `TextLayout` | `crates/nana-text/src/layout/ir.rs:182` | 保留布局 IR：`runs` / `lines` / `bounds` / `overflow` / `objects` / `rubies` / `labels` |
| `LineBox` / `LineMetrics` | `crates/nana-text/src/layout/ir.rs` / `metrics.rs` | 单行运行区间与度量（行宽、行高、基线偏移） |
| `TextLayoutStore` / `TextLayoutId` | `crates/nana-text/src/layout/store.rs:54` | 代际化句柄的保留布局仓库 |
| `FontSystem` | `crates/nana-text/src/font/system.rs` | 字体数据库：注册、世代递增、匹配、实例与 `cmap` 覆盖率回退 |
| `FontGeneration` / `TextRevision` | `crates/nana-text/src/id.rs` | $O(1)$ 失效判定的两个单调整数：字体库代际与源文本修订号 |
| `IntrinsicWidths` | `crates/nana-text/src/layout/engine.rs:109` | `min-content` 与 `max-content` 固有宽度测量结果 |

## 内部结构

### 1. 一条调用串起三段：`NativeTextEngine::layout`

`TextEngine::layout(kind, source, base, constraints, counters) -> Arc<TextLayout>`（`crates/nana-text/src/engine.rs:36-43`）返回的是不可变的共享 `Arc`——相同文本与相同约束在后续帧直接命中缓存拿到同一份指针。其内部实现（`engine.rs:294-378`）按固定六步推进：

1. **`fold_lines`（`:192`）**：在 `white-space: normal` 下将源文本中的换行符折叠为空格——整形器与断行器必须看到完全相同的字节流，因此折叠发生在整形**之前**；
2. **`shape`（`:178`）**：结合 `base.language` 或引擎默认语言构造 `ShapeRequest`，交给 `Shaper` 产出 `Arc<ShapedText>`；
3. **省略号整形（`:203`）**：将 `const ELLIPSIS: &str = "…"`（`:23`）走标准整形路径，使同一样式下的所有截断节点共享同一份省略号缓存；
4. **`strut_metrics`（`:218`）**：计算基础样式自身字体面的支柱度量（strut），用于锁住最小行盒高度与基线，防止同行内更高的回退字形把基线顶歪（按 `(FontInstanceKey, canonical_f32_bits(size))` 缓存在容量为 `1024` 的 `BoundedCache` 中，`:76`）；
5. **注音（ruby）与内联对象标签整形（`:305-341`）**：各自按比例字号整形；
6. **行排版与计数差分（`:343-376`）**：组装 `LayoutRequest` 交给 `Layouter::layout`，并将本趟前后的缓存命中/未命中、解析字形数、断行机会数差值写入 `TextWorkCounters`。

### 2. 字体层：注册、世代与 `cmap` 覆盖率回退

`FontSystem`（`crates/nana-text/src/font/system.rs:1-31`）维护三条核心保证：

- **字节生命周期与句柄退役解耦**：字体字节封装为 `FontBlob`（`Arc`）；注销一个字体面只退役其 `FontId` 句柄，不会夺走后台正在读取的字节（`:4-12`）。
- **双向精确的代际失效（`FontGeneration`）**：每次注册、注销、装载系统字体或切换回退策略都会使 `FontGeneration` 加一（`:18-31`）。选择缓存整体清空（因为新注册的字体可能改变任何字体族列表的胜出者），而 `cmap` 覆盖率缓存只精准移除被卸载的那些字体面；已退役 `FontId` 的槽位代际永不复用。
- **确定性选择与诊断可解释性**：`FontQuery`（族列表、字重、字宽、斜度、语言）结合 `FontOrigin` 优先级（显式注册压过系统字体扫描，`:61-79`）与 `FaceDescriptor`（`@font-face` 声明的族名覆盖字体内部表名，`:82-90`）选出字体面，并记录 `FontChoiceReason` 与 `FontAssignment`，使诊断工具能精确回答「某段文字为什么落到了这个回退字体上」。

### 3. 整形层：分段、BiDi、HarfRust 与有界回退预算

`Shaper`（`crates/nana-text/src/shaping/shaper.rs:27-34`）先将文本源切分为**样式段（`StyleSegment:43`）**，再按同样式、同 BiDi 嵌入层级、同 Unicode Script、同直立/侧卧取向切分为**项（`Item:51`）**，项内再按字体面 `cmap` 覆盖率切分为**片（`Piece:64`）**（`U+FFFC` 内联对象占位符永不进入整形器，`:58`）。

为防止一段生僻或损坏字符耗尽所有系统字体，回退设有双层硬预算：`MAX_FALLBACK_CANDIDATES_PER_RANGE = 8`（单个缺字区间最多尝试 8 个候选字体面）、`MAX_FALLBACK_RETRIES_PER_ITEM = 32`（单项最多重试 32 次，`shaper.rs:20-24`）。竖排文本在整形期即按 UAX #50 拆分为直立运行（启用字体竖排度量 `vert` / `vrt2`）与侧卧运行（按横排整形、绘制时顺时针旋转 90°）。

### 4. 行布局层：与整形解耦、UAX #14 折行与 `TextLayout` IR

`layout/mod.rs`（`crates/nana-text/src/layout/mod.rs:4-17`）的核心设计是**将行布局与 OpenType 整形彻底解耦**：`Layouter` 只读取已整形的 `ShapedText` 并按行切片组合，绝不重新调用 HarfRust。因此**当窗口拖拽缩放改变容器宽度、切换折行模式、修改 `max_lines` 或改变水平对齐时，只需重跑轻量的行布局模块，完全复用已有的整形缓存**。

每行的结束原因由 `LineBreakCause`（`ir.rs:110-121`）精确记录：

| 枚举变体 | 含义 |
| --- | --- |
| `Explicit` | 源文本中的显式换行符 `\n` |
| `Wrap` | 行宽预算用尽触发自动折行 |
| `MaxLines` | 达到 `max_lines` 行数上限截断 |
| `MaxHeight` | 下一行装不进 `max_height_px` 高度预算而截断 |
| `EndOfText` | 自然到达文本末尾 |

在竖排书写（`vertical-rl` / `vertical-lr`）下，行排版在「行相对坐标系」中与横排复用同一套代码，全引擎只有三处感知页面转了向：行预算取容器高度而非宽度（`TextConstraints::inline_budget_px`）、行盒使用中央基线而非字母基线、以及最终的物理坐标映射（`physical_x_of_block` / `physical_size`，`ir.rs:170-176, :240`）。布局是否过期只需做一次 $O(1)$ 双整数比较：`layout.is_stale(revision, font_generation)`（`:310-312`）。

### 5. `TextLayoutStore`：保留句柄与缓存的分界

普通 LRU/FIFO 缓存会随时自动驱逐条目，而挂在保留树节点上的布局需要一个「随节点存续、随节点销毁释放、跨世界或旧代际访问时明确报错拒绝」的句柄——这正是 `TextLayoutStore`（`crates/nana-text/src/layout/store.rs:54`）的职责。槽位代际取自全进程单调序列 `NEXT_GENERATION`（`:9-15`），`resolve` 时同时核对槽位代际与 `FontGeneration`（过期返回 `StaleLayout::Retired` 或 `StaleLayout::FontGeneration`）。

## 协作与数据流

`UiWorld` 通过 `paint_text_engine: Option<nana_text::SharedTextEngine>`（`crates/nana-ui-runtime/src/world.rs:677`）持有进程级共享引擎，并在 `NodeStore` 中将 `text_nodes: HashMap<StableNodeId, TextNodeState>` 单独建表（`store.rs:436-444`），使大范围布局能快速跳过无文本变更的节点。在 `resolve_plain_text`（`world/text.rs:4197-4251`）中，若截断后的布局带有 `OverflowFlags::ELLIPSIZED` 且父盒需要自然宽度，Runtime 会再用无界约束查询一次自然布局以获取未折行前的真实最大宽度。最终保留在节点上的 `TextLayoutId` 随 `ExtractedNode` 交给 `UiScene`，由 `NanaRenderer::text` 直接读取字形运行并栅格化上屏。

## 代码地图

| 文件路径 | 核心内容 |
| --- | --- |
| `crates/nana-text/src/lib.rs` | 门面导出、阶段演进文档与 12 个 `nana-ui-core` 排版词汇白名单 |
| `crates/nana-text/src/engine.rs` | `TextEngine` / `NativeTextEngine` / `SharedTextEngine`：三段流水线总入口 |
| `crates/nana-text/src/font/system.rs` | `FontSystem`：字体注册、世代递增与精确缓存失效 |
| `crates/nana-text/src/font/{matching,fallback,variations,coverage,face,discovery,unicode,query}.rs` | 字体面选择、`cmap` 覆盖率回退、可变字体轴、系统字体扫描與 Unicode 属性 |
| `crates/nana-text/src/shaping/{shaper,bidi,opentype,cache,key}.rs` | 整形分段、BiDi 层级、HarfRust 封装、`ShapeCache` 与缓存键 |
| `crates/nana-text/src/layout/{engine,lines,breaks,ir,store,cache,key}.rs` | 行布局求解、UAX #14 断点、`TextLayout` IR 与 `TextLayoutStore` |
| `crates/nana-text/src/edit.rs` / `editable/` | 光标定位、命中测试推导与可编辑分段会话 |

## 延伸阅读

- [核心合同](/learn/core-contracts)：被本 crate 借用的排版词汇在 `nana-ui-core` 中的定义
- [布局引擎](/learn/runtime-layout)：文本固有尺寸测量如何与盒级布局反馈收敛
- [保留树：UiWorld](/learn/runtime-tree)：`TextNodeState` 与 `NodeStore` 稀疏侧表的组织方式
- [架构：文本](/architecture/text) · [文本引擎参考](/reference/text-engine)：面向应用与维护者的文本规范
