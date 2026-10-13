---
title: CSS 引擎:nana-ui-css
---

# CSS 引擎:nana-ui-css

这一章下钻 `nana-ui-css`:整个框架唯一的 CSS 引擎,负责样式表解析、选择器匹配、级联,以及把命中的声明映射到 Style Model(`nana_ui_core::LayoutStyle`)。读完你能搞懂:一份 CSS 从字符串到 `LayoutStyle` 字段的完整内部流水线(手写词法 → 选择器 → 级联 → 映射);`@media`/`@import`/`@layer`/`@font-face`/`@keyframes`/`@container` 各自被编译成什么;为什么同一个引擎有两个消费时机——`view-codegen` 构建时与 Vue 运行时——却产出同一份布局数据;以及 `tools/css-parity` 如何用夹具夹住它与浏览器行为的一致性。

## 职责与边界

`lib.rs` 文件头定调:"NanaUI's CSS subset, in one place"(`crates/nana-ui-css/src/lib.rs:1`)。样式表是子集内的普通 CSS——选择器、级联、定制属性、`@media`、`@import`、`@font-face`、`@keyframes`、交互伪类——全部映射到唯一的 Style Model(`crates/nana-ui-css/src/lib.rs:3-6`)。

- **唯一引擎**:解析、级联、映射三件事只在这一个 crate 做;"CSS parsing stays out of `nana-ui-core`, `nana-ui-runtime`, `nana-ui-scene` and `nana-ui`"(`crates/nana-ui-css/src/lib.rs:13-14`)。
- **不测量容器**:`@container` 被编译成数据交给运行时求值,"this crate never measures a container"(`crates/nana-ui-css/src/lib.rs:6-8`;`crates/nana-ui-css/src/css_container.rs:3`)。尺寸永远归 Runtime 布局这一个权威。
- **不做布局求解**:产出是 `LayoutStyle` 意图数据;flex/grid 实际求解在 Runtime(见[运行时布局](/learn/runtime-layout))。
- **子集即边界**:子集清单由 `docs/reference/layout.md` 列出;子集外的声明不猜测,记入 `UnsupportedCssReport` 报告。
- **输入可信度有限**:样式表与字体字节都有尺寸上界(`MAX_STYLESHEET_BYTES`/`MAX_FONT_FACE_BYTES`/`MAX_REGISTERED_FONT_BYTES`),`@import` 有深度上界与 `is_blocked_href` 检查(`crates/nana-ui-css/src/lib.rs:34-39`)——引擎按不可信输入对待样式表。

## 依赖与邻居

`crates/nana-ui-css/Cargo.toml` 依赖只有 `nana-ui-core` 与 `serde_json`(:14-15)——它是典型 L2 crate。消费方恰好两个(工作区内搜 `nana-ui-css` 依赖,`Cargo.toml` 命中仅此两处):

| 消费者 | 时机 | 做什么 |
| --- | --- | --- |
| `nana-ui-view-codegen`(`crates/nana-ui-view-codegen/Cargo.toml:11`) | 构建时 | `.vue` 编译器解析并匹配样式,把结果编译成数据,L3 视图运行时零解析 |
| `nana-ui-vue`(`crates/nana-ui-vue/Cargo.toml:46`) | 运行时 | Vue 路径在运行时解析 + 级联(经 `nana-ui-vue` 再导出给 `tools/css-parity` 用) |

`nana-ui-runtime` 与 `nana-ui` 的依赖表里没有 `nana-ui-css`:运行时不跑 CSS 引擎,样式以 Style Model 数据形式进入(见[核心合同](/learn/core-contracts))。

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `StyleRule` | `crates/nana-ui-css/src/css_cascade.rs:103` | 一条规则:选择器列表 + 声明条目 + source_order + layer |
| `Selector` / `CompoundSelector` | `crates/nana-ui-css/src/css_cascade.rs:114` / `:150` | 复合选择器链与单个复合(组合器 `Combinator`:125) |
| `Specificity` | `crates/nana-ui-css/src/css_cascade.rs:262` | 三元组 `u16`×(ids / classes+attrs / types) |
| `MediaEnv` | `crates/nana-ui-css/src/css_cascade.rs:424` | 媒体求值环境,`@media`/`@import` 条件对它求值 |
| `MatchContext` / `MatchNode` | `crates/nana-ui-css/src/css_cascade.rs:291` / `:278` | 匹配现场:祖先链、前驱兄弟、:has 位图、媒体环境 |
| `DeclarationEntry` | `crates/nana-ui-css/src/css_cascade.rs:86` | 一条声明:属性、值、`!important` |
| `StylesheetParseReport` / `UnsupportedCssTally` | `crates/nana-ui-css/src/css_cascade.rs:481` / `:563` | 解析报告与子集外内容的累计统计 |
| `RuleIndex` | `crates/nana-ui-css/src/css_cascade.rs:1298` | 按 type/id/class 键的候选规则索引 |
| `ParsedStylesheet` | `crates/nana-ui-css/src/css_interactive.rs` | 解析结果分桶:静态规则 / 交互规则 / 生成伪元素 / motion / @container |
| `KeyframesRule` / `MotionStyleRule` | `crates/nana-ui-css/src/css_interactive.rs` | 动效桶:关键帧(百分比停点)与 motion 声明,待编译成轨道 |
| `ContainerRule` / `ContainerQuery` | `crates/nana-ui-css/src/css_container.rs` | @container 编译产物:轴 + 半开区间集合 |
| `InteractiveStyleRule` | `crates/nana-ui-css/src/css_interactive.rs` | `:hover`/`:focus`/`:active` 规则,静态级联不应用 |
| `FontFaceRule` / `FontFaceSrc` | `crates/nana-ui-css/src/css_at_rule.rs` | @font-face 的 src 与描述符;装载走 `StylesheetLoader`(`MemoryStylesheetLoader`/`PackagedStylesheetLoader`,`lib.rs:34-37`) |
| `LayoutStyleCss` | `crates/nana-ui-css/src/css_map.rs:1862` | 映射入口 trait:`apply_css_property` 等三个方法 |
| `CssPaintColor` | `crates/nana-ui-css/src/style.rs:30` | CSS 侧颜色,`to_core()` 换 `nana_ui_core::PaintColor`(:51) |

## 内部结构

### 词法与解析:手写规则读取器

没有外部解析器:读入、切分、识别全在手写函数里。一份样式表走过的站:

1. `strip_css_comments`(:3037)去注释;
2. `split_rule`(:3091,在第一个 `{` 处切开选择器与块)、`split_selector_list`(:3132)按逗号分选择器,`skip_at_rule`(:3061)跳过(或分发)@-规则;
3. 选择器进 `parse_selector`(:2024)→ `tokenize_selector_chain`(:2339)→ `parse_compound`(:2433),声明块进 `parse_declaration_entries`(:1422)切成 `DeclarationEntry`;
4. 结果按 `ParsedStylesheet` 分桶(静态/交互/生成伪元素/motion/@container)。

入口一族按能力递增:`parse_stylesheet`(:658)→ `parse_stylesheet_full`(:663)→ `_with_layers`(:671)→ `_with_options`(:680),底层 `parse_stylesheet_full_with_cache`(:701)带解析级缓存。输入有上界:`MAX_STYLESHEET_BYTES`、`MAX_IMPORT_DEPTH`、`MAX_FONT_FACE_BYTES` 等常量(`crates/nana-ui-css/src/lib.rs:34-36`)。

@-规则的分发在 `parse_known_at_rule`(`crates/nana-ui-css/src/css_cascade.rs:738`):`@import` 经 `apply_import`(:858)就地展开(深度/layer/media 保留),`@layer` 名字经 `record_layer_names`(:945)登记,`@media` 变成条件规则,`@font-face` 解析为 `FontFaceRule`,`@keyframes` 按百分比停点收集(`crates/nana-ui-css/src/css_at_rule.rs:1-6`)。

解析不是静默吞错:`StylesheetParseReport`(:481)汇总一趟解析的发现,`UnsupportedCssReport`(:507)记录子集外构造,`UnsupportedCssTally`(:563)做累计统计——子集是显式清单,越界内容有账可查,而不是被悄悄丢弃。

### 选择器模型与匹配

匹配是「上下文驱动」的:`MatchContext` 携带节点的 tag/id/classes/attrs、祖先链(父在前)、前驱兄弟(紧邻兄在前)、兄弟计数、预计算的 `has_bits:u64`、`focus_within`、`is_empty`、`checked` 与媒体环境(`crates/nana-ui-css/src/css_cascade.rs:291-344`)。`selector_matches`(:1957)自右向左走:`Child` 沿祖先严格上爬,`Descendant` 扫祖先,`AdjacentSibling`/`SubsequentSibling` 走前驱列表;文档注明「父节点的前驱兄弟不在上下文里——fail closed」(:1975-1977)。

fail-closed 是这个引擎的通用姿态:`:has()` 的实参 `has_args` 为空时所有 `:has()` 不命中(:291 注释);竖排书写、复杂选择器上下文缺失时宁可拒绝。特异性是标准三元组(`Specificity`:262),候选集由 `RuleIndex`(:1298)按 type/id/class 键缩小——候选是严格超集,最终仍由 `selector_matches` 验证,查询代价是 O(候选) 而非 O(全部规则)。

选择器细节子集也不含糊:`:nth-child` 一族解析成 `AnPlusB`(:193,`parse_an_plus_b`:2980),属性选择器解析成带 `AttrOperator`/`AttrCase` 的 `AttrSelector`(:223/:242/:253,`parse_attr_inner`:2666),`:is()`/`:where()` 风格的选择器列表由 `parse_simple_selector_list`(:2742)处理,列表的特异性按 `max_alts_specificity`(:2918)取备选中最大者。生成伪元素(`::before` 等)与滚动条伪元素在解析期就从主体剥离开单独建规则(`parse_generated_pseudo_selector`:2115、`parse_scrollbar_pseudo_selector`:2126),伪类交互规则由 `parse_interactive_selector`(:2035)单独成桶。

### 级联与定制属性

`matched_declaration_entries_from`(:1253)对命中规则排序,次序是(important, specificity, source_order, decl_index)(:1275-1280);一条规则只需一个选择器命中即取(:1271)。层叠层的键由 `cascade_layer_key(important, layer)`(:444)统一编序;`@media` 由 `media_list_matches`(:454)对 `MediaEnv`(:424)求值;`@supports` 条件的求值器 `evaluate_supports_condition` 与媒体查询族一起从 `css_at_rule.rs` 再导出(`crates/nana-ui-css/src/lib.rs:33-40`)。

重建一个元素样式的完整次序写在 `rebuild_layout_style`(:1606):样式表 → 类提示 → 属性样式文本 → 内联文本 → 样式表 `!important` 补一轮 → 属性/内联 `!important` → 逻辑盒边解析。方向与书写模式条目先于其余声明应用(:1701-1712),因为它们决定后续逻辑属性映射到哪条物理边。索引版 `rebuild_layout_style_indexed`(:1647)用 `RuleIndex` 候选;`@container` 的额外规则"join the stylesheet layers…never appended after it"——它们的 `!important` 并入样式表 important 层,属性/内联 `!important` 仍能压过它们。

定制属性(自定义属性)走 `collect_document_custom_properties_from_rules`(:1458)与 `matched_custom_properties_indexed`(:1520):按 `:root`/`[data-theme]` 作用域收集成映射,`var()` 在应用前展开。多张已解析的样式表可经 `merge_parsed_stylesheet`(再导出于 `crates/nana-ui-css/src/lib.rs:59-66`)合并成一份桶结构,宿主按窗口或主题组合样式时不必重新解析文本。

### 映射到 Style Model:LayoutStyleCss

`crates/nana-ui-css/src/css_map.rs`(9100+ 行)是属性到 `LayoutStyle` 字段的映射表,入口是 trait `LayoutStyleCss`(:1862):`apply_class_layout_hints`、`apply_css_text(style, percent_w, percent_h)`、`apply_css_property` 三个方法,`impl` 给 `nana_ui_core::LayoutStyle`(:1880-1907)。已知属性落到对应字段(`apply_css_text_orientation`:3202 这类一族小函数);值解析有 `parse_css_length_px`(:5721,带百分比基)、`parse_css_font_size`(:4629)、`resolve_paint_color`(:4617);未知属性进 `UnsupportedCssReport`。`var()` 展开由 `with_active_css_vars` 上下文携带(view-codegen 调用处见 `crates/nana-ui-view-codegen/src/style.rs:220`)。

颜色有自己的通道:`CssPaintColor`(`crates/nana-ui-css/src/style.rs:30`)解析 CSS 颜色语法,`to_core()`(:51)换成 core 的 `PaintColor`,不丢 Oklch/Hsv 精度。L1 颜色政策在此执行:`is_non_token_css_color`(:648)识别「不是 token 名的颜色」,`map_css_color_for_tokens`(:635)只把已知 token 名映射成语义角色——与 core 的 `SemanticColorRole::from_css_token_name` 政策呼应。

### @规则与属性子集

一张表先看清各 @-规则被编译成什么:

| @-规则 | 编译产物 | 去处 |
| --- | --- | --- |
| `@import` | 就地展开的规则(深度/layer/media 保留) | `css_cascade.rs:858` `apply_import` |
| `@layer` | 层号(layer 0 起序) | `css_cascade.rs:945` `record_layer_names` |
| `@media` | `MediaQueryList` + `MediaEnv` 求值 | `css_at_rule.rs` `evaluate_media_query_list` |
| `@font-face` | `FontFaceRule`(src/weight/style/stretch 描述符) | 宿主注册进 `nana-text` 字体层 |
| `@keyframes` | `KeyframesRule`(百分比停点) | Motion 侧编译成轨道 |
| `@container` | `ContainerQuery`(轴 + 半开区间集合) | 运行时按容器实测尺寸选桶 |

- **交互伪类**(`crates/nana-ui-css/src/css_interactive.rs:1-6`):`:hover`/`:focus`/`:active` 在解析期落入 `InteractiveStyleRule` 桶,静态级联**不应用**,等宿主/运行时接线后再生效;`:focus-visible` 映射为 `Focus`(引擎没有键盘/指针信号,只在聚焦时匹配,:21-25)。`::before`/`::after` 物化为生成盒;`::placeholder`/`::selection` 只是绘制层颜色;`::-webkit-scrollbar`(-thumb) 是皮肤覆盖不是盒(:68-98)。
- **@container**(`crates/nana-ui-css/src/css_container.rs`):查询编译为 `ContainerQuery`——容器名、轴(`ContainerAxis`:53)、`ContainerInterval` 半开区间集合(:64-82)。运行时三步:`ContainerRuleSet::matching` 找到命中的容器规则,`plan_container_queries` 把容器区间按查询边界切成至多 `MAX_CONTAINER_BREAKPOINTS + 1`(=17,:47-49)个桶,再由 `rebuild_layout_style_indexed` 把每个桶的规则与样式表一起级联(:10-19)。子集:width/height/inline-size/block-size、`min-`/`max-`、区间与布尔形式;其余(`aspect-ratio`、`style()`、嵌套 @container 等)记为 `ContainerQueryUnsupported` 永不应用(:32-38)。
- **motion**(`crates/nana-ui-css/src/css_motion.rs:1-4`):`transition` 简写解析为 `TransitionShorthand`(:10-46),时长全为 0s 时整体放弃(:37-39);Vue 路径在运行时把结果编成动画轨道,`.vue` 编译器编成隐式动画。缓动映射到 `nana_ui_core::Easing`。
- **工具类**(`crates/nana-ui-css/src/shell_contract.rs:1-8`):`gap-sm`(4px)/`gap-md`(8px)/`gap-lg`(16px)、`p-*`、`w-full`、`flex-1`、`items-*`、`overflow-*`、`rounded*` 这类**非中立**的宿主类在此显式罗列(:20-90);业务 BEM(`lilia-*`)明确不得收割。为什么单列一处:文件头写明 "Neutral CSS declaration parse stays in `crate::css_map`"——中立语法与「Nana 自家类名词表」是两件事,`LayoutStyleCss::apply_class_layout_hints` 委托到这里,让级联/测量的调用点保持稳定(`crates/nana-ui-css/src/shell_contract.rs:7-8`)。

### 两个消费时机:构建时与运行时

- **构建时**(`crates/nana-ui-view-codegen/src/style.rs:239` `parse()`):`parse_stylesheet_full(css, 0)` 解析,`sheet_vars`(:184)取表内定制属性,`compile_rules`(:204)把每条规则编译成补丁——`patch()`(:192)在空 `LayoutStyle` 上依次 `apply_css_property` 再 diff 出被写字段,按 (important, specificity, source_order) 定 rank,产出 `Rule { classes, rank, patch, query }`(:227-232);`@container` 块的规则共享样式表 source_order,带桶索引(:244-259)。于是 L3 视图把样式当**数据**携带,运行时零解析。
- **运行时**(`crates/nana-ui-vue/src/bridge/cascade.rs:763`):Vue 适配器对元素现场调 `rebuild_layout_style_indexed` 重建 `LayoutStyle`;样式表加载用 `parse_stylesheet_full_with_options`(:1816)。`nana-ui-vue/src/lib.rs:323-327` 把整套引擎再导出给宿主与工具。

性能有专门基准盯着:`crates/nana-ui-vue/src/bin/nana-css-l1-benchmark.rs` 用 `parse_stylesheet` 预热规则表(:134),再对 `apply_stylesheet_to_layout` 逐元素计时(:217-269),覆盖解析与级联两条热路径。

## 协作与数据流

两条消费路径汇于同一模型:同一份匹配声明,构建时与运行时产出相同 `LayoutStyle`(`crates/nana-ui-css/src/css_cascade.rs:1-4`)。以 Vue 运行时一次元素重样式为例:

1. 样式表加载时 `parse_stylesheet_full_with_options` 解析一次(`crates/nana-ui-vue/src/bridge/cascade.rs:1816`),规则与定制属性映射常驻;
2. 元素重样式时,桥接层从保留树取 tag/id/classes/祖先/兄弟组装 `MatchContext`,经 `RuleIndex` 候选 + `selector_matches` 验证,`rebuild_layout_style_indexed` 级联出 `LayoutStyle`(`:763`);
3. `@font-face` 解析出的 `FontFaceRule` 交给字体层注册(见[文本引擎](/learn/text-engine)的 `FaceDescriptor`);`@keyframes`/motion 声明交给 Motion 侧编译成轨道(见[核心合同](/learn/core-contracts)的 Motion IR);
4. 产出的 `LayoutStyle` 被 `measure_layout` 与绘制端消费——引擎到此为止,不测量、不画。

**与 `tools/css-parity` 的关系**:这个工具(`tools/css-parity/Cargo.toml`,包名 `nana-css-parity`)用夹具夹住「声明 → 盒子」的全链路。夹具是 JSON:一棵带内联样式的树、视口、期望盒子和 `tolerance_px` 容差(`tools/css-parity/fixtures/T-B01.json`:`display:flex; padding:1px 2px 3px 4px` 期望 `inner` 落在 (4,1));lib 侧 `fixture_to_layout_node` 组出 `LayoutNode`,`measure_nana` 调 `nana_ui_vue::measure_layout`(`tools/css-parity/src/lib.rs:13,134-136`),`compare_to_expected`(:141)对期望逐盒比对。CLI `cargo run -p nana-css-parity -- compare|list|html`(`tools/css-parity/src/bin/css_parity.rs:1-86`);149 个夹具按 T-F(flex)/T-S(尺寸与 calc)/T-B(盒模型)/T-G(grid)/T-I(内联)/T-L(布局)/T-P(定位)/T-FL(浮动)分族,清单在 `tools/css-parity/src/cases.rs` 与 `docs/reference/layout.md` 的子集对齐(:1-20)。与真实 WebView 的对照不在本 crate,归 `tools/css-parity-webview`(bin 头注释 `tools/css-parity/src/bin/css_parity.rs:3-4`);`parse_webview_boxes`(`tools/css-parity/src/lib.rs:243`)负责把 WebView 量出的盒子读回同一比对格式,`fixture_to_html`(:190)则把夹具渲染成 HTML 供另一侧测量——同一个期望格式服务两端。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-css/src/css_cascade.rs` | 引擎主体:解析、选择器、匹配、级联、RuleIndex(5600+ 行) |
| `crates/nana-ui-css/src/css_map.rs` | 属性 → `LayoutStyle` 映射与值解析(9100+ 行) |
| `crates/nana-ui-css/src/css_at_rule.rs` | @media/@import/@layer/@font-face/@keyframes 的头部解析与求值 |
| `crates/nana-ui-css/src/css_container.rs` | @container 编译:区间、桶、plan |
| `crates/nana-ui-css/src/css_interactive.rs` | 解析期分桶:交互伪类、生成伪元素、keyframes、motion |
| `crates/nana-ui-css/src/style.rs` | CSS 颜色解析与 token 政策 |
| `crates/nana-ui-css/src/css_motion.rs` | transition 简写解析 |
| `crates/nana-ui-css/src/shell_contract.rs` | 宿主工具类(gap-sm 等,非中立部分) |
| `crates/nana-ui-view-codegen/src/style.rs` | 构建时消费者:规则 → 数据补丁 |
| `crates/nana-ui-vue/src/bridge/cascade.rs` | 运行时消费者:现场级联 |
| `tools/css-parity/` | 一致性夹具与 CLI |

## 延伸阅读

- [核心合同](/learn/core-contracts):`LayoutStyle` 与 Style Model 三层,映射的目的地
- [运行时布局](/learn/runtime-layout):拿到 `LayoutStyle` 之后 flex/grid 如何求解
- [文本引擎](/learn/text-engine):`@font-face` 注册后的字体匹配与整形
- [创作路径](/learn/authoring-paths):写 `.vue` 时构建期/运行期样式各走哪条路
- [工具链](/learn/toolchain):view-codegen 与 parity 工具在构建管线中的位置
- [参考:how-it-works](/reference/how-it-works):从样式到帧的全链路参考
- [架构:帧](/architecture/frame):一帧内样式解析与测量的时点
- [覆盖矩阵](/learn/coverage-matrix)与[证据清单](/learn/evidence-checklist):本章论断对应的夹具与测试入口
