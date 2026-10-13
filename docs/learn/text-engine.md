---
title: 文本引擎:nana-text
---

# 文本引擎:nana-text

这一章下钻 `nana-text`:产品的文本测量、整形(shaping)与保留文本布局权威。读完你能搞懂:一次 `TextEngine::layout` 调用里「测量 → 整形 → 保留布局」三段各做什么、边界在哪;字体层如何用世代(Generation)与 cmap 覆盖率决定「每个簇用哪张脸」;整形如何分段、跑 BiDi、调 HarfRust 并按预算回退;布局为什么与整形解耦,让一次窗口尺寸风暴只花行布局的钱;Runtime 如何经 `TextLayoutStore` 句柄保留每个文本节点的布局,以及为什么它是产品权威而不是对系统 UI 工具包的依赖。

## 职责与边界

`lib.rs` 按 Issue 编号自述家谱:自 Phase 1(#90)拥有字体层(注册、世代、脸匹配、变体坐标、覆盖驱动回退),Phase 2(#91)拥有整形(分段、BiDi 层级、HarfRust 整形、有界整形缓存),Phase 3(#92)拥有行布局(单行 Label 快路径、UAX #14 断行机会上的贪心折行、逐行视觉重排、行盒度量、对齐、省略号、有界布局缓存),`NativeTextEngine` 把三段收在 `TextEngine` 一个调用后面;自 Phase 4(#95)UiWorld 的保留文本节点经它解析,布局留在 `TextLayoutStore` 句柄后(`crates/nana-text/src/lib.rs:7-18`)。

- **唯一产品文本权威**(自 #99):"Rust, NanaVue and Vue/CSS text is measured by it, and `nana-ui`'s `NanaRenderer::text` draws the layouts the Runtime retained"(`crates/nana-text/src/lib.rs:18-21`)。AGENTS.md 同款合同:nana-text 是产品文本测量/整形/保留布局权威。
- **不是系统工具包的包装**:`cosmic_text`/`cryoglyph` 的标识符不得出现在 `src/` 下——记录 Phase 0 金标准的那台 cosmic 参考引擎已在 #99 删除,金标准留下,原生引擎对着它核对;`scripts/check-engine-boundary.py` 把两个引擎都挡在产品依赖图外(`crates/nana-text/src/lib.rs:25-29`)。成熟实现(fontdb/skrifa/harfrust/unicode-bidi/unicode-linebreak)只藏在私有模块里,公共 API 不点名它们(`crates/nana-text/Cargo.toml:21-37`)。
- **词汇只借不重造**:`TextStyle`/`TextConstraints` 用 `nana_ui_core` 的排版类型搭成,白名单只有 `FontVariationSetting`、`FontKerningSpec`、`LineBreakSpec`、`FontFeatureSetting`、`LineHeightSpec`、`WordBreakSpec`、`TextWrapBreak`、`TextAlignSpec`、`DirSpec`、`WritingModeSpec`、`TextOrientationSpec`、`WritingContext` 十二个(`crates/nana-text/src/lib.rs:30-37`)——UiWorld 适配器因此是逐字段搬运而不是十几套手写枚举转换。
- **不做编辑**:光标、选择、命中测试是成品 IR 上的推导(`crates/nana-text/src/edit.rs`,`crates/nana-text/src/layout/mod.rs:48-49`);`justify` 延后未做(核心 `TextAlignSpec` 没有 justify 关键词,`crates/nana-text/src/layout/mod.rs:37-38`);竖排编辑未建,编辑器要竖排时按横排落布局并计数上报(`:39-47`)。
- **变体与字距按 OpenType 实义消费**:变体设置、字距、断行、书写方向等词汇全部来自 core 的中性枚举(见[依赖与邻居](#依赖与邻居)的白名单),本 crate 不发明第二种「差不多」语义。

## 依赖与邻居

`crates/nana-text/Cargo.toml` 依赖表就是「L2 + 私有成熟实现」的样本:`nana-ui-core`、`serde`/`serde_json`、`unicode-segmentation` 之外,fontdb 0.24(系统字体发现,只许 `font/discovery.rs` 点名)、skrifa 0.44(变体轴/命名实例/色彩表/cmap,只许 `font/face.rs`)、icu_properties 2.3(Script/Emoji_Presentation,只许 `font/unicode.rs`)、harfrust 0.12(OpenType 整形,只许 `shaping/opentype.rs`)、unicode-bidi 0.3.18(只许 `shaping/bidi.rs`)、unicode-linebreak 0.1.5(UAX #14,只许 `layout/breaks.rs`)(`crates/nana-text/Cargo.toml:16-37`)。

| 邻居 | 关系 |
| --- | --- |
| `nana-ui-core` | 排版词汇与 `WritingContext` 的来源(白名单见上) |
| `nana-ui-runtime` | 最大消费者:经 `SharedTextEngine` 测量,经 `TextLayoutStore` 保留(见[协作与数据流](#协作与数据流)) |
| `nana-ui` | `NanaRenderer::text` 绘制 Runtime 保留的布局,不再自行排版 |
| `nana-ui-css` | `@font-face` 解析出描述符,由宿主注册进字体层 |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `TextEngine` | `crates/nana-text/src/engine.rs:25` | 三段管线的接缝:trait 只有两个方法 `font_generation` 与 `layout` |
| `NativeTextEngine` | `crates/nana-text/src/engine.rs:79` | 字体系统 + Shaper + Layouter + 省略号源 + 语言提示 + strut 缓存 |
| `SharedTextEngine` | `crates/nana-text/src/engine.rs:50` | `Arc<Mutex<NativeTextEngine>>`:同字体集的一切共享一台引擎 |
| `TextSource` / `TextStyle` / `TextKind` | `crates/nana-text/src/source.rs` / `src/style.rs:28` / `:16` | 带跨度与内联对象的文本源、基础样式、文本种类(Label/Paragraph/Editable…) |
| `TextConstraints` | `crates/nana-text/src/constraints.rs:30` | 容器约束:max_width/height、wrap 算法、max_lines、ellipsis、书写模式、对齐、tab 宽度、缩放 |
| `ShapedText` / `ShapedRun` | `crates/nana-text/src/shaping/` / `src/shape.rs` | 整形产物:不可变字形运行,带方向/脚本/取向 |
| `LayoutRequest` | `crates/nana-text/src/layout/engine.rs:33` | 一次布局的全部输入:shaped(只读)、约束、省略号、strut、ruby/标签 |
| `TextLayout` | `crates/nana-text/src/layout/ir.rs:182` | 保留布局 IR:runs/lines/bounds/overflow/objects/rubies/labels |
| `LineBox` / `LineMetrics` | `crates/nana-text/src/layout/ir.rs` / `src/metrics.rs` | 一行的运行区间与度量(宽/高/基线),`lib.rs:79` 一并再导出 |
| `BoundedCache` | `crates/nana-text/src/bounded.rs`(经 `engine.rs:93` 使用) | 有界缓存底座:strut 按 (实例, 尺寸) 存取就是它 |
| `TextLayoutStore` / `TextLayoutId` | `crates/nana-text/src/layout/store.rs:54` | 世代化句柄的保留布局仓 |
| `FontSystem` | `crates/nana-text/src/font/system.rs` | 字体数据库:注册、世代、选择、实例、覆盖回退 |
| `FontGeneration` / `TextRevision` | `crates/nana-text/src/id.rs` | 失效的两个整数:字体库世代 / 源文本修订 |
| `SharedText` / `TextStamp` | `crates/nana-text/src/shared.rs`(`lib.rs:81` 再导出) | 跨节点共享的去重文本与时间戳 |
| `TextEngineEpoch` | `crates/nana-text/src/engine.rs:65` | 引擎有效性:font_system + font_generation + language_generation |
| `IntrinsicWidths` | `crates/nana-text/src/layout/engine.rs:109` | min-content / max-content,容器定宽前的测量 |

## 内部结构

### 一条调用串起三段:NativeTextEngine::layout

`TextEngine::layout(kind, source, base, constraints, counters) -> Arc<TextLayout>`(`crates/nana-text/src/engine.rs:36-43`)的结果是共享而非持有的:布局不可变,同文本同约束的下一帧必须拿到缓存里那一份。实现(`engine.rs:294-378`)按固定次序:

1. `fold_lines`(:192):`white-space: normal` 下作者换行按空格折——整形与断行必须看到同一批字节,所以折行发生在整形*前*;
2. `shape`(:178):按 `base.language` 或引擎语言提示构造 `ShapeRequest`,交 `Shaper`;
3. 省略号经普通路径整形一次(:203,`const ELLIPSIS: &str = "…"`,:23)——同一样式的所有截断节点共享一份缓存;
4. `strut_metrics`(:218):基础样式自己那张脸的度量,strut 钉住行盒,让一行里更高的回退字形挪不动基线;按 (FontInstanceKey, size) 缓存,`STRUT_CAP = 1024`(:76);
5. ruby 注音与对象标签各自按份额样式整形(:305-341);
6. 组装 `LayoutRequest` 交 `Layouter::layout`(:343-348),沿途差分计数器写进 `TextWorkCounters`。

计数器不是装饰:一趟 layout 前后各读一次引擎计数,差出本趟的整形缓存命中/未命中、布局缓存命中/未命中、解析出的字形数、行机会数、行断比较数、竖排回退数与被哈希的文本字节数(:295, :352-376)——诊断页与回归测试读的就是这些数,「一趟没有文本工作」也要能被证明。

`intrinsic_widths`(:161-176)是同一管线的窄化:折行、整形后问 Layouter 要 min/max-content。

### 字体层:注册、世代与覆盖驱动回退

`FontSystem` 的文件头把合同说全(`crates/nana-text/src/font/system.rs:1-31`):

- **字节生命周期**:脸的字节是 `FontBlob`(`Arc`);`face_data` 发克隆,注销一张脸只退役句柄,不夺走 worker 还在读的字节;系统脸首次使用时从磁盘读入后同样持有(:4-12)。strut 缓存的键也按此精神做成精确键:(`FontInstanceKey`, `canonical_f32_bits(size)`)——浮点尺寸经规范化位型入键,相邻尺寸不共享答案(`crates/nana-text/src/engine.rs:231`)。
- **世代**:`crates/nana-text/src/font/system.rs:18-31` 每次变异(注册、注销、换源、装载系统字体、换回退策略)把 `FontGeneration` 加一;选择与实例携带自己被算出的世代(:22);失效双向精确——选择缓存全清(加任何脸都可能改变族列表解析结果),覆盖率条目只删被移走那些脸(别的脸的 cmap 没变),退役 `FontId` 的槽位世代永不复用,按 `FontInstanceKey` 键控的缓存不会别名为替换脸。
- **确定性输出**:这一层不整形,`resolve_text` 只按 cmap 覆盖率说「每个簇应该用哪张脸」,整形仍可对整形出 `.notdef` 的簇重试(`crates/nana-text/src/font/mod.rs:4-7`)。
- **匹配与变体**:`FontQuery`(族列表、字重、宽度、斜度、语言)选脸;`FontVariations` 把变体设置解析成轴坐标(WGHT/WDTH/SLNT/ITAL 在 `variations.rs`);`FaceDescriptor` 承接 CSS `@font-face` 描述符——声明的 family **替换**脸自身名字参与匹配(`system.rs:82-90`)。注册来源 `FontOrigin`(System/File/Memory)有决胜 rank,显式注册压过系统扫描(:61-79)。
- **模块分工是合同的一部分**:`system.rs` 的 use 清单点名了各司其职(`crates/nana-text/src/font/system.rs:33-43`)——`matching.rs` 出 `MatchFace`/`StyleSupport`,`fallback.rs` 出 `FallbackPolicy`/`FontAssignment`/`FontChoiceReason`,`coverage.rs` 管 cmap 覆盖缓存与预算(`DEFAULT_COVERAGE_BUDGET_BYTES`,`font/mod.rs:23` 再导出),`discovery.rs` 负责系统字体扫描,`face.rs`/`unicode.rs` 是 skrifa 与 icu 的封装位。选择带理由(`FontChoiceReason`)、结果成赋值(`FontAssignment`),上层诊断能回答「这段字为什么用了这张脸」。

### 整形:分段、BiDi、HarfRust 与回退预算

`Shaper`(`crates/nana-text/src/shaping/shaper.rs:27-34`)持 `ShapeCache`、`FaceShapers` 与 `FontEpoch`——世代变了缓存整体作废。管线先把源切成**样式段**(`StyleSegment`:43),再切成**项**(`Item`:51):一段同样式、同 BiDi 层级、同脚本、同取向的最大区间,项内再按脸覆盖切成**片**(`Piece`:64),每片带已试过的脸清单。U+FFFC 是内联对象占位,永不整形(:58)。整形请求经 `ShapeRequest::new(source, base, constraints).with_language(...)` 组装(`crates/nana-text/src/engine.rs:184-186`)——语言提示逐请求生效,而不是引擎级一刀切。

回退有预算:`MAX_FALLBACK_CANDIDATES_PER_RANGE = 8`(一个缺字区间最多试 8 张脸)、`MAX_FALLBACK_RETRIES_PER_ITEM = 32`(每项最多重试 32 次),合起来保证「一段没有脸能覆盖的文本」不会花掉 文本长度 × 脸数(`shaper.rs:20-24`)。段落分隔符(`PARAGRAPH_SEPARATORS`)整形期丢弃——整形丢掉的必须恰好是 BiDi 收段的位置,否则会画成 `.notdef`(:71-83)。竖排文本在整形期就按 UAX #50 拆成直立运行(用字体竖排度量,自上而下整形)与侧躺运行(按横排整形,绘制时旋转),见 `crates/nana-text/src/layout/mod.rs:21-26`。

### 保留布局:UAX #14 断行、视觉重排与 TextLayout IR

`layout/mod.rs` 头部给出流水线(`crates/nana-text/src/layout/mod.rs:4-12`):ShapedText + TextConstraints → 单行 Label 快路径,或 → 段落文本上的 UAX #14 断行机会 → 按**已整形步进**贪心折行 → 每行按 BiDi 规则 L1/L2 视觉重排 → 行盒度量、对齐、省略号 → 不可变 `TextLayout`,按 `LayoutKey` 缓存。快路径与完整路径产出同一种 IR,消费者不区分。

**整形与布局解耦**是性能设计的关键:布局读整形运行、按行复制所需,绝不重整形;"Changing the width, the wrap mode, `max_lines` or the alignment re-runs *this* module only, which is what makes a resize storm cost line work rather than shaping work"(:14-17)。`LayoutRequest` 上写明 shaped 是**被读的**(`crates/nana-text/src/layout/engine.rs:28-32`);引擎侧把行装配拆给 `lines.rs` 的 `Builder`/`LineInput`/`LineStrut`/`segments`(`crates/nana-text/src/layout/engine.rs:7`),断行机会只从私有的 `breaks.rs` 触碰 `unicode-linebreak`。未设 line-height 时行盒按 `DEFAULT_LINE_HEIGHT_RATIO = 1.2`——与 core 的 `text_line_box_height_px` 同一个数,两处不吵架(:20-26)。

`TextLayout`(`crates/nana-text/src/layout/ir.rs:182-217`)字段:revision、font_generation、constraints、runs、lines、bounds、overflow、unsupported_writing_mode、objects(被截断的对象不上榜)、rubies、labels。每行结束的原因是枚举 `LineBreakCause`(:110-121):

| 变体 | 含义 |
| --- | --- |
| `Explicit` | 源里的 `\n` |
| `Wrap` | 行宽用尽 |
| `MaxLines` | `max_lines` 在此截断 |
| `MaxHeight` | `max_height_px` 截断:下一行装不进盒 |
| `EndOfText` | 文本结束 |

装不下的事实由 `OverflowFlags` 位标记(:125-130,如 `TRUNCATED_LINES`);内联对象落位记为 `PlacedObject`(id、U+FFFC 偏移、所在行、行空间矩形,:62-69),ruby 注音与对象标签各有 `PlacedRuby`/`PlacedLabel`(自带已整形的 runs 与绘制原点,:94-105)。几何是**行相对**的:x 沿行、y 跨行堆叠;竖排时 x 沿列下行、y 自块首列数起,全 IR 只有 `physical_x_of_block`/`physical_size` 一处知道页面转了向(:170-176, :240, :269)。行宽取「最长一行」而非 bounds 并集——居中对齐的行起点在盒内,把右缘当宽度会让收缩盒被自己上一帧的宽度定尺寸(:262-268)。失效判断是 O(1) 两个整数:`is_stale(revision, font_generation)`(:310-312)。

竖排实现还有一个模块级视角:「只有三处知道页面翻了」——给行做预算的是盒子的哪个维度(`TextConstraints::inline_budget_px`)、行盒用中央基线还是字母基线、最终映射;竖排行本体在行相对坐标系里与横排**同一套代码**排版,整形侧按 UAX #50 拆直立/侧倒运行(`crates/nana-text/src/layout/mod.rs:19-29`)。这把「竖排支持」从横贯引擎的特性收敛成三个明确的插拔点。

### 编辑:IR 上的推导层

光标、命中与编辑不回写引擎状态:「**Caret and editing**: those are derivations on the finished IR ([`crate::edit`]), not engine state」(`crates/nana-text/src/layout/mod.rs:48-49`)。crate 再导出两族类型(`crates/nana-text/src/lib.rs:65-69`):`edit` 出 `Affinity`/`CaretPosition`/`CaretStop`/`HitTestResult`/`CaretGeometry`,`editable` 出 `EditSession`/`EditState`/`EditSelection`/`Motion`/`TextEdit` 等会话态。命中测试落在行空间坐标上(`TextLayout::hit_test` 读 `line_space_point` 的逆映射,`crates/nana-text/src/layout/ir.rs:254-260`),Runtime 的编辑组件持同一台 `SharedTextEngine` 做几何(`crates/nana-ui-runtime/src/text_engine_shaper.rs:52-69`)——编辑是布局的消费者,不是第二台引擎。

### 缓存与世代:从 ShapeCache 到 TextLayoutStore

四层缓存各有失效口径:ShapeCache 按内容哈希 + `FontEpoch` 世代;布局缓存按 `LayoutKey`;strut 缓存按 (实例, 尺寸) 且世代变即清,容量上界 `STRUT_CAP = 1024`(`crates/nana-text/src/engine.rs:76, :93`);`TextLayoutStore`(`crates/nana-text/src/layout/store.rs:54`)是「保留」与「缓存」的分界——缓存自行驱逐,保留布局需要一个随节点存续、随节点释放、错了就被拒绝而不是被悄悄改指向的句柄(:1-7)。槽位世代取自全进程序列 `NEXT_GENERATION`,所以句柄只被发它的那个仓认(:9-15);`resolve` 额外拒绝另一字体世代下产的布局,`StaleLayout::Retired` 或 `StaleLayout::FontGeneration { layout, current }`(:36-45, :105-118)。引擎层面 `TextEngineEpoch`(font_system/font_generation/language_generation 三元组,:65-71)覆盖语言提示变化——它像注册一张脸一样影响 `locl` 整形与回退。

## 协作与数据流

Runtime 是这台引擎的最大消费者,接口集中在四点:

- **共享引擎**:`UiWorld` 持 `paint_text_engine: Option<nana_text::SharedTextEngine>`(`crates/nana-ui-runtime/src/world.rs:677`);每次解析加锁、绝不跨趟持有——同一趟里组件文本可能经宿主 `shape` 又回到这台引擎(`crates/nana-ui-runtime/src/world/text.rs:4221-4223`)。锁中毒直接恢复:每次缓存插入是单操作,读不到半应用状态(`crates/nana-text/src/engine.rs:52-59`)。
- **世代对表**:语言提示的变化同样计入有效性——`TextEngineEpoch` 三元组里 `language_generation` 与字体世代并列(`crates/nana-text/src/engine.rs:65-71`),因为语言到达 `locl` 整形与字体回退的方式和注册一张脸一样。
- **保留句柄**:`NodeStore` 里 `text_nodes: HashMap<StableNodeId, TextNodeState>`(修订号、解析戳、布局句柄)单独立表——大范围重排要对每个候选快速判「无文本工作」,小表扫描留在缓存里(`crates/nana-ui-runtime/src/store.rs:436-444`);`text_layouts: nana_text::TextLayoutStore`(:444)随节点释放。
- **解析路径**:`resolve_plain_text`(`crates/nana-ui-runtime/src/world/text.rs:4197-4251`)按 `TextShapeConstraints` 调 `layout`,若布局报 `OverflowFlags::ELLIPSIZED` 且盒要求截断,再用无界约束要一次**自然布局**取真实文本宽度(:4233-4251)——折过的行宽等于盒宽,收缩盒要知道文本未折时多宽。
- **绘制**:`nana-ui` 的 `NanaRenderer::text` 画的就是 Runtime 保留的这些布局(`crates/nana-text/src/lib.rs:20-21`);编辑侧另有 `text_engine_shaper.rs` 持引擎做光标几何(`crates/nana-ui-runtime/src/text_engine_shaper.rs:52-69`)。
- **窄接口**:`text_width.rs` 是「只要宽度」的入口——同样锁引擎、同样 `layout`,取度量后即弃(`crates/nana-ui-runtime/src/text_width.rs:76`);组件上下文的 `text_engine()` 访问器开在同一处(`crates/nana-ui-runtime/src/components.rs:2432`);抽取阶段 `world/extraction.rs:509` 把引擎可选地传进场景抽取。

为什么它是产品权威:一台引擎被同字体集的一切共享——"every document and window of a process, and the painter that rasterizes what they laid out. Sharing it is what lets one label's shaping serve every other copy of it"(`crates/nana-text/src/engine.rs:46-49`);失效是整数比较而非重新指纹文本(`lib.rs:41-44`);跨 Rust/NanaVue/Vue 三条路径同一套度量(`lib.rs:19-20`);而迁移正确性由 `parity` 模块的结构 diff 与语料把守(`lib.rs:5-7`,`crates/nana-text/src/parity/`)。可观测性也内建:`reset_counters`/`shape_counters`/`layout_counters`(`crates/nana-text/src/engine.rs:145-157`)让上层定期采走整形/布局/字体三侧计数,配合 `TextWorkCounters` 的每趟差分,「这一帧有没有做多余的文本工作」是可回答的问题——这也是它不外包给系统工具包的实际理由之一:外包装不出这种口径。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-text/src/engine.rs` | `TextEngine`/`NativeTextEngine`:三段的缝合点 |
| `crates/nana-text/src/font/system.rs` | `FontSystem`:注册、世代、失效策略 |
| `crates/nana-text/src/font/mod.rs` | 字体层门面与再导出:「公共面不出现 fontdb/skrifa」的执行点 |
| `crates/nana-text/src/font/{matching,fallback,variations,coverage,face,discovery,unicode,query}.rs` | 选脸、覆盖回退、变体轴、cmap、系统发现、脚本属性 |
| `crates/nana-text/src/shaping/{shaper,bidi,opentype,cache,key}.rs` | 整形管线、BiDi、HarfRust 封装、缓存与键 |
| `crates/nana-text/src/layout/{engine,lines,breaks,ir,store,cache,key}.rs` | 布局权威、断行、IR、保留仓 |
| `crates/nana-text/src/{constraints,style,source,metrics,shape}.rs` | 约束、样式、源、度量、运行 IR |
| `crates/nana-text/src/{bounded,id}.rs` | 有界缓存底座;`TextLayoutId`/`FontGeneration` 等标识 |
| `crates/nana-text/src/lib.rs` | 门面:阶段文档、边界、core 词汇白名单、再导出 |
| `crates/nana-text/src/edit.rs` / `editable/` | 光标/命中推导与编辑会话(非引擎状态) |
| `crates/nana-text/src/parity/` | 迁移 parity:结构 diff 与语料 |

## 延伸阅读

- [核心合同](/learn/core-contracts):被本 crate 借用的排版词汇在 core 的定义
- [运行时布局](/learn/runtime-layout):文本节点测量如何汇入盒级布局求解
- [运行时保留树](/learn/runtime-tree):`TextNodeState` 与 `NodeStore` 侧表的组织
- [CSS 引擎](/learn/css-engine):`@font-face` 描述符如何到达字体层
- [架构:文本](/architecture/text):面向应用作者的文本路径描述
- [参考:how-it-works](/reference/how-it-works):文本在一帧里的解析与绘制时点
- [支撑 crate](/learn/supporting-crates):nana-text 在六个 L2 中的同侪
- [证据清单](/learn/evidence-checklist):文本行为该用什么测试证明
