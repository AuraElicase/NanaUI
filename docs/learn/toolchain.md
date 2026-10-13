---
title: 作者工具链
---

# 作者工具链

作者写下的 `view!`/`css!`/`stylesheet!` 与 `.vue` 文件,在**应用编译期**就被编译成 nana-ui-runtime 的声明式视图函数调用与 Style Model 数据;JS 面则是「一份引擎合同 + 一个 V8 实现 + 一组宿主 API」。这一章把两条编译管线和 JS 面逐个拆开:模板去哪、样式去哪、脚本去哪,每一步落在哪个 crate 的哪一行。

## 职责与边界

| Crate | 一句话职责 | 明确不做的事 |
| --- | --- | --- |
| nana-ui-view-macros | `view!`/`css!`/`stylesheet!` 过程宏入口,只解析模板语法 | 不做代码生成(交给 view-codegen) |
| nana-ui-view-schema | 内建控件唯一描述表(schema 中间表示) | 不含任何生成逻辑 |
| nana-ui-view-codegen | 模板元素树 → runtime 声明式视图函数;构建时调 CSS 引擎编译样式 | 头文档:*"不新增任何运行时概念"* |
| nana-ui-sfc | `.vue` 单文件组件(Rust script)构建时编译器 | 不进运行时;产物是普通 Rust 源码 |
| nana-js-engine | `JsEngine` trait 与 `HostApiRegistry` 合同(测试注入缝) | 不做绘制、不碰保留树 |
| nana-js-v8 | V8 对合同的实现 | 未开 `engine` feature 时全是 stub |
| nana-ui-web-api | JS 侧 window/document/fetch/计时器等宿主 API | 不是 WebView、不是第二条绘制路径 |

两条编译管线的共同纪律:**运行时收到的是数据,不是代码**。`crates/nana-ui-runtime/src/view/style.rs:1-27` 的模块文档写明:`.vue` 编译器(和 `css!`)在应用构建时用 nana-ui-css 解析并匹配样式;到运行程序的是「每元素按级联顺序适用的规则,各为 StylePatch + 条件类」——运行时不解析 CSS、不匹配选择器,runtime 里没有 CSS 引擎。

## 依赖与邻居

层级号沿用[模块与分层](/learn/modules-and-layers)的依赖深度归组(每条普通边指向更小层级,一条 optional 同层边例外):

| Crate | 层级 | 关键依赖(normal) | 备注 |
| --- | --- | --- | --- |
| nana-ui-view-schema | 0 | — | 零依赖的纯表 |
| nana-js-engine | 0 | — | 零依赖合同 |
| nana-ui-css | 2 | nana-ui-core | 唯一 CSS 引擎,被下面两条管线共用 |
| nana-ui-view-codegen | 3 | nana-ui-core, nana-ui-css, nana-ui-view-schema | 依赖注释:*"Styles are parsed and matched with the engine the Vue path runs"* |
| nana-js-v8 | 3 | nana-js-engine | `engine` feature 才链 v8 152.2.0 |
| nana-ui-web-api | 4 | nana-js-engine, nana-ui-core, nana-ui-platform, nana-text, nana-svg-raster | 头文档自称 **L1** 渐进兼容级(与工作区分层的 L4 是两套编号) |
| nana-ui-view-macros | 4 | nana-ui-view-codegen | proc-macro crate |
| nana-ui-sfc | 5 | nana-ui-view-codegen | runtime 不在其依赖表里,见下文 |

两个容易误读的边界:

- **sfc 与 runtime 的关系是产物指向,不是编译依赖**。`crates/nana-ui-sfc/Cargo.toml:9-17` 的 normal 依赖只有 nana-ui-view-codegen 加 prettyplease/proc-macro2/quote/serde/syn;runtime 以**路径字符串**进入生成代码(`Compiler::new`,见 `crates/nana-ui-sfc/src/lib.rs:294`,传入形如 `::nana_ui::runtime` 的路径)。生成的代码引用 runtime,编译 sfc 本身不需要它。
- **js-v8 的重依赖都是 dev-dependencies**(nana-ui-vue、nana-ui、nana-ui-web-api、wgpu、pollster 等,见 `crates/nana-js-v8/Cargo.toml` 的 `[dev-dependencies]`):它们只出现在 js-v8 自己的测试里,不进任何消费者。

## 关键类型与数据

| 类型 | 定义处 | 作用 |
| --- | --- | --- |
| `Control`(字段 tag/function/arguments/fields/events) | `crates/nana-ui-view-codegen/src/lib.rs:18` | schema 表展开后的控件描述 |
| `CONTROLS: &[Control]` | `crates/nana-ui-view-codegen/src/lib.rs:53-77` | 由 `for_each_control!` 展开的常量表 |
| `Node`/`Element`/`Attr`/`TextPart` | `crates/nana-ui-view-codegen/src/lib.rs:333-405` | 模板中间表示(元素树) |
| `CompiledStyles` | `crates/nana-ui-view-codegen/src/style.rs:978` | 构建时样式编译产物:每节点 patch + 条件类 |
| `Rank = (bool, (u16,u16,u16), u32)` | `crates/nana-ui-view-codegen/src/style.rs:49` | 级联序:!important、specificity、源顺序 |
| `StyleWarning { at, message }` | `crates/nana-ui-view-codegen/src/style.rs:87` | 未编译进产物的样式(非静默丢弃) |
| `Component { name, props, script, template, style }` | `crates/nana-ui-sfc/src/parse.rs:23` | `.vue` 文件解析后的四大块 |
| `Compiler { runtime, hot }` | `crates/nana-ui-sfc/src/lib.rs:115` | SFC 编译器配置 |
| `HostApiRegistry` | `crates/nana-js-engine/src/lib.rs:1068-1072` | host ops 注册表(BTreeMap + 异步表 + 观察器) |
| `HostValue` | `crates/nana-js-engine/src/lib.rs:287` | JS ↔ Rust 值 ABI |
| `RuntimeArtifact` | `crates/nana-js-engine/src/lib.rs:486` | 加载进引擎的代码包 |
| `WebApiState` | `crates/nana-ui-web-api/src/lib.rs:75` | 宿主 API 的可变状态(localStorage/rAF/计时器/fetch/ws) |

## 内部结构

### 管线一:Rust 宏链(`view!` / `css!` / `stylesheet!`)

四个过程宏都定义在 `crates/nana-ui-view-macros/src/lib.rs`,但作者从 runtime 侧使用:`crates/nana-ui-runtime/src/view/mod.rs:134` 在 `view-macro` feature 下再导出 `pub use nana_ui_view_macros::css as __css`;`stylesheet!` 是 runtime 的 `macro_rules!` 包装,转发时先塞进 `crate = $crate;` 路径(`crates/nana-ui-runtime/src/view/mod.rs:151-162`),过程宏用 `parse_crate`(`crates/nana-ui-view-macros/src/lib.rs:117`)读出,经 proc-macro-crate 3 解析成真实 crate 名——这样展开代码里的 `column(...)` 总指向正确路径。

| 宏 | 入口 | 产物 |
| --- | --- | --- |
| `view!` | `crates/nana-ui-view-macros/src/lib.rs:17` | 模板元素树交给 view-codegen,展开成 `column/text/each/when/...` 声明式视图函数调用 |
| `Store` derive | `crates/nana-ui-view-macros/src/lib.rs:24` | 生成 `<Name>StoreFields` trait,每字段一个 accessor |
| `css!` | `crates/nana-ui-view-macros/src/lib.rs:41` | CSS token 写的声明块,构建时调 CSS 引擎,产物给 `El::css` 用 |
| `stylesheet!` | `crates/nana-ui-view-macros/src/lib.rs:76` | `mod styles;` + 规则 → 一个持有 sheet、每 class 一个 `view::Class` 的模块 |

`css!` 用 CSS 的 token 语法而非 Rust 表达式;Rust tokenizer 无法处理的值写成双引号字符串,如 `font-size: "1.5em"`。传错时 `crates/nana-ui-view-macros/src/lib.rs:47` 直接报 "`css!` takes CSS, not a string"。`stylesheet!` 生成的 class 名把 `-` 换成 `_`;编译警告放在 `const` 里,因为 item 位置放不下 `let`(`crates/nana-ui-view-macros/src/lib.rs:76` 文档)。runtime 侧的用法(`crates/nana-ui-runtime/src/view/mod.rs` 的模块文档示例):

```rust
stylesheet! {
    mod styles;
    .card { /* … */ }
}
widget(card).class(styles::card).class_when(styles::done, done)
```

——`styles::card` 就是编译期产出的 `view::Class`,条件类在运行时只是布尔开关,不重新解析任何 CSS。

**schema:一张表喂两个消费者。** `crates/nana-ui-view-schema/src/lib.rs:49` 的 `for_each_control!` 回调宏是内建控件的唯一描述,表格式为:

```text
Tag => element_function(argument: kind, …) for RustType {
    field: FieldType = write,
} [on { … }] [with { … }] [model field => EventType |event| …];
```

头文档(`crates/nana-ui-view-schema/src/lib.rs:1-8`)写明双消费:runtime 展开成 typed setter、`model` 与事件方法;view-codegen 展开成它接受的 tag 与属性——所以拼错的属性是模板错误并指向所在行,而在这里加一个控件,两边同时获得。`write` 描述值如何落进组件(同文件 23-40 行):`set`(同名字段)、`text_state`(编辑文本仅差异时替换,保光标)、`assign`(数字经 assign,新值把光标放后面)、`clamp`(范围钳制)、`span_low`/`span_high`/`span`(双拇指,不许交叉)、`builder`(经组件 builder 方法,保住派生量)、`tab`(选中并取焦点)、`resource`(图像宿主纹理槽)。argument 的 kind 是 `text`/`f32`/`f64`/`expr`。表里是 Text、Button、Checkbox、Switch、Slider、TextInput、TextArea、NumberInput、Select、Tabs、TreeView、IconButton、Texture、ColorField(`value: [f32;4] = builder`)、ActionMenuItem 等 26 个控件(`crates/nana-ui-view-schema/src/lib.rs:52-226`)。

**codegen:展开与构建时 CSS。** view-codegen 用 `table` 宏把 schema 表变成 `pub const CONTROLS: &[Control]`(`crates/nana-ui-view-codegen/src/lib.rs:53`,调用点 `:77`);`is_builtin`(`:87`)另认结构 tag:T/Column/Row/Widget/Block/Virtual/Transition/TransitionGroup/KeepAlive/Suspense/Teleport/ErrorBoundary。三个展开入口 `expand`/`expand_checked`/`expand_checked_at`(`:287/:300/:310`)分别对应无位置检查、检查、带 span 检查——SFC 用带 span 的那个,错误才能指回 `.vue` 文件行号。`lift_slots`(`:407`)把槽位声明提升到合法位置;`localized_text`(`:142`)把插值模板转成 `LocalizedText::new("files").arg("count", n)`;`STYLE_ROLES`(`:326`)枚举 `["foreground","background","border","radius"]` 四个样式角色。

样式编译集中在 `crates/nana-ui-view-codegen/src/style.rs`(1504 行),三个入口:`compile_inline`(css! 内联,`:1111`)、`compile_styles`(SFC `<style>`,`:986`)、`compile_stylesheet`(stylesheet!,`:1004`)。它构建时调用 nana-ui-css(`parse_stylesheet_full`/`written_layout`/`plan_container_queries` 等),只匹配**类选择器**(`.card`、`.card.active`)到节点的 `class`/`class:name` 绑定,按 `Rank` 排级联序,把每条规则变成 Style Model 字段值的 patch;`transition` 声明变成元素隐式动画;`@container` 块内的类规则带上查询数据(容器、轴、区间),运行时按容器尺寸桶组合;其余——其他选择器、`:hover`、`@media`、`@keyframes`、不支持的查询、无字段声明——一律产出 `StyleWarning`,**不静默丢弃**(`crates/nana-ui-view-codegen/src/style.rs` 头文档)。

### 管线二:`.vue` 单文件组件(nana-ui-sfc)

`.vue` 文件的 script 是 Rust:`<script setup lang="rust"> defineProps!(todo: Todo, …)`,模板走与 `view!` **同一个**代码生成器。`TodoItem.vue` 编译成 `pub fn todo_item(...) -> impl IntoView`(`crates/nana-ui-sfc/src/lib.rs:543` 附近生成)。三块各自去处:

| 块 | 解析 | 去处 |
| --- | --- | --- |
| 模板 | `crates/nana-ui-sfc/src/parse.rs:23` 存进 `Component.template` | `Rewrite.nodes` → `lift_slots` → `expand_checked_at`(view-codegen) |
| 样式 | `parse.rs:33` `Style { css, line, column }`,`position()` 把 CSS 偏移换回文件行列 | `compile_styles` 对着模板类绑定匹配成 patch(顺序见下) |
| 脚本 | `Component.script: Vec<Stmt>` | 信号分析 → 折叠 → 与生成函数拼接 |

`crates/nana-ui-sfc/src/lib.rs:413` 的 `component()` 是整条流水:parse → `Analysis::new` + `scan_script` → count → 环检查 → fold → finish → `compile_styles`(刻意放在「分析见过条件类之后、绑定重写之前」)→ 重写节点 → `lift_slots` → `expand_checked_at` → `fold_stmt`。脚本分析是这条管线的独特价值(头文档,`crates/nana-ui-sfc/src/lib.rs:1` 起):编译器看全组件,知道哪些名字是信号——无人写的 `signal` 折成 `constant`(`let x = signal(v)` 变 `constant(v)`,`Signal<T>` 注解变 `Const<T>`);只读常量的 `computed` 也折叠;只写一次的绑定不生成 effect;`computed` 互相读成环是**错误**("computeds read each other in a loop: a → b");写而不读的 signal 与读写自身的 watcher 是警告;每个绑定的类与依赖写进报告。

集成方式是 build script:`Compiler::build`(`crates/nana-ui-sfc/src/lib.rs:389`)把整个目录编译成三件产物:

| 产物($OUT_DIR 下) | 内容 |
| --- | --- |
| `nana_views.rs` | 全部视图函数的 Rust 源码,应用 `include!` 进自己的模块 |
| `nana_views.deps.md` | 依赖清单(哪个视图来自哪个 `.vue`) |
| `nana_views.map.json` | 源映射(遵循 `nana-sfc-source-map/1`) |

编译时打印 `cargo:rerun-if-changed` 与 `cargo:warning`,examples 里的 reactive-sfc 就是这个形态(*"Views written as .vue files with Rust scripts, compiled by nana-ui-sfc in build.rs"*,其 Cargo.toml description)。错误定位:每条脚本语句前插一个 `__NANA_SFC_MARKER__|file|line|col` const;`Error { file, line, column, message }` 的行列是 1-based(`crates/nana-ui-sfc/src/lib.rs:51`);`Output.source_map` 遵循 `SOURCE_MAP_SCHEMA = "nana-sfc-source-map/1"`(`:85`)。表达式解析用了一个朴素技巧:`parse_at`(`crates/nana-ui-sfc/src/parse.rs:101`)以 `"\n".repeat(line-1) + " ".repeat(col) + text` 的 padding 让 proc-macro2 的 span 落回原文件坐标。

开发体验上有 `hot` 模式(`Compiler::hot`,`crates/nana-ui-sfc/src/lib.rs:306`):静态文本读一张可替换表(`__NANA_HOT: &[&str]`),改文案不用重编译,由 nana-ui-dev 的 `watch_templates` 驱动(见[外围支撑 crate](/learn/supporting-crates));每个视图带 FNV-1a `stable_hash` 的形状哈希(`:135`,`HotView { name, shape, literals }` 在 `:123`),模板结构变了仍要重建。

### JS 面:引擎合同、V8 实现、宿主 API

**nana-js-engine(L0)是合同与注入缝。** `pub trait JsEngine`(`crates/nana-js-engine/src/lib.rs:1254-1345`)头文档一句话定界:*"Unified JS engine contract used by nana-ui-vue. Paint and retained UI stay in Runtime/UiScene; this trait is JS execution only."* 核心方法:`initialize(RuntimeArtifact)` 装载代码包;`register_host_api(&HostApiRegistry)` 把宿主 API 装成 `globalThis.__nanaHost.call(name, args)`(文档建议先于 `initialize`);`resolve_function(name) -> JsFunctionId` + `invoke(id, &[HostValue]) -> HostValue` 执行 JS;`run_microtasks` 排空 Vue nextTick/Promise 与各 realm 的 host 调用;`create_realm`/`dispose_realm` 与 `*_in` 变体支撑多窗口多 realm(默认实现报 "this JS engine does not support separate realms");另有 `host_event_sender`、`host_resources`、`interrupt`/`request_gc`/`shutdown`。

`HostApiRegistry`(`crates/nana-js-engine/src/lib.rs:1068-1072`)是 host ops 的唯一注册处:同步 `register`/`register_arc` 与异步 `register_async`/`register_async_arc`(`:1084-1125`)互斥,后者顶掉同名前者;`try_extend`(`:1133`)预检冲突后原子合并,重名报 "duplicate host API name";异步名被同步 `call`(`:1177`)时报 "`host API `{name}` is asynchronous; use Nana.host.invoke`"。每次调用可选经过 `HostCallTrace` 观察器(`:622`)记下 name/asynchronous/pending/succeeded/duration_micros。这个 registry 同时是**测试注入缝**:nana-ui-vue 的 `renderer.rs` 从 2000 行起的大量 `#[test]` 直接 `HostApiRegistry::new()` 注册 DOM host ops 断言行为,不需要起 V8。crate 还有默认关闭的 `probe-fixtures` feature(`:1347-1398`),内置约 576KB 的探针 bundle(`VUE_RUNTIME_PROBE_JS` 等)供引擎级测试。

`JsEngine` 的方法按用途分组:

| 组 | 方法 | 说明 |
| --- | --- | --- |
| 装载 | `initialize(RuntimeArtifact)` / `register_host_api(&HostApiRegistry)` | 文档建议注册先于 initialize |
| 执行 | `resolve_function(name) -> JsFunctionId` / `invoke(id, &[HostValue]) -> HostValue` | 按名字解析一次,反复调用 |
| 事件循环 | `run_microtasks()` | 排空 Vue nextTick、Promise 与各 realm 的 host 调用 |
| realm | `create_realm` / `dispose_realm` / 各 `*_in` 变体 | 多窗口多 realm;默认实现报 "this JS engine does not support separate realms" |
| 观测 | `host_event_sender` / `host_resources` | 事件出口与宿主资源登记 |
| 生命周期 | `interrupt` / `request_gc` / `shutdown` | 中断、建议 GC、关停 |

**nana-js-v8(L3)是产品实现。** `crates/nana-js-v8/src/lib.rs`(62 行)整个 crate 在 `engine` feature 之后:不开时 `V8Engine` 全方法是 stub,调用报 "nana-js-v8: enable feature `engine` to link crates.io v8 152.2.0"。真实现 `crates/nana-js-v8/src/engine.rs`(4384 行)里,`RealmHost { api, bridge, resources, events }`(`engine.rs:22`)存进 V8 context slot,让原生回调找到自己该服务的 realm;`DiagnosticsSlot` 挂 isolate 级 message/promise-rejection 回调,转成 js-engine 的 `JsDiagnosticEvent`。crate 还带 `nana-v8-snapshot` bin(`required-features = ["engine"]`)。

**nana-ui-web-api(L4)是宿主 API 面。** 头文档(`crates/nana-ui-web-api/src/lib.rs:1-5`)自我定界:*"buffered WebView-source compatibility subset, not a WebView or a second paint path"*——JS 跑在 Rust 拥有的 V8,可见输出仍走 Runtime/UiScene → `SceneWgpuPainter`。构成:

- build 时生成的 `WEB_API_SHIM_JS`(`include_str!($OUT_DIR/shim.js)`,`:54`)把 window/document/localStorage/rAF/history 装到 globalThis;`compose_runtime_artifact`(`:65`)把 shim 与应用 bundle 拼成一个 `RuntimeArtifact` 一次装载。
- `WebApiState`(`:75`)持全部可变状态(localStorage 适配、pending rAF、timeouts/intervals、document dataset/style、location、`FetchRuntime`、`SocketRuntime`)。
- 注册面是一族函数(`:508-553`):`register_web_api_host_ops` 及 `_with_resources`/`_with_clipboard`、`register_audio_host_ops`、`register_media_host_ops`、`register_clipboard_host_ops`——全都吃 `&mut HostApiRegistry`。
- rAF 没有宿主帧时以 `RAF_FRAME_INTERVAL = 16ms` 兜底(`:41`)。
- fetch/WebSocket/剪贴板的具体行为 re-export 自 nana-ui-platform 的 `FetchHost`/`WebSocketHost`/`ClipboardHost` 族(如 `NativeFetchHost`、`MemoryClipboard`)。Canvas2D 用自带封闭字体集(nana-text + swash,不看宿主注册字体);Canvas 的 SVG Image 经 nana-svg-raster 的 `rasterize_document`。`native-audio` feature(默认关)才链 cpal。

host ops 的完整拼图(注册进同一个 registry):nana-ui-vue 的 `register_dom_host_ops`(`crates/nana-ui-vue/src/renderer.rs:43`,DOM/Custom Renderer)、`startup.rs` 的框架启动 API(文档注明重名即启动失败,`crates/nana-ui-vue/src/startup.rs:17`)、`register_input_host_ops`(`crates/nana-ui-vue/src/host/input_projection.rs:36`)、WebGPU 互操作(`crates/nana-ui-vue/src/webgpu.rs:343`)、每窗口独立 realm 的注册(`crates/nana-ui-vue/src/multi_window.rs:1105`),加上 web-api 的兼容层,以及应用经 `hosted_adapter` 的 `application_api: HostApiRegistry` 注入的自有 API。

## 协作与数据流

```text
管线一  view!/css!/stylesheet! ──view-macros(解析)──▶ view-codegen(展开+构建时 CSS)──▶ runtime 视图函数 + StylePatch 数据
管线二  .vue 文件 ──sfc parse──▶ 模板→view-codegen;样式→compile_styles;脚本→信号分析折叠 ──▶ build.rs 产物 nana_views.rs
JS 面   app bundle + shim ──▶ RuntimeArtifact ──initialize──▶ V8(nana-js-v8)◀──HostApiRegistry(合同:nana-js-engine)──▶ vue/web-api/应用 host ops
```

- **CSS 的两个时机**:同一段 CSS,经 `view!`/`.vue`/`stylesheet!` 走构建时(view-codegen 调引擎,产物是 patch);经 Vue 运行时路径在 nana-ui-vue 里跑同一个 nana-ui-css 引擎级联。判断样式在哪解析,看入口不看内容(详见[CSS 引擎](/learn/css-engine)与[三种作者入口](/learn/authoring-paths))。
- **JS 的输出边界**:host ops 写进 `PendingHostOps`(`UiMutation` 队列),帧边界 commit 进 `UiWorld`;绘制永远是 Runtime → UiScene → SceneWgpuPainter,JS 侧没有第二条路(端到端见[端到端数据流](/learn/flows))。
- **诊断接线**:js-v8 的 isolate 回调 → `JsDiagnosticEvent` → devtools 的 `js_sink()`(见[外围支撑 crate](/learn/supporting-crates))。

## 代码地图

| Crate | 入口 | 规模 |
| --- | --- | --- |
| nana-ui-view-macros | `crates/nana-ui-view-macros/src/lib.rs`(+`css_tokens`/`store` 模块) | 685 行 |
| nana-ui-view-schema | `crates/nana-ui-view-schema/src/lib.rs` | 236 行(一张表) |
| nana-ui-view-codegen | `crates/nana-ui-view-codegen/src/lib.rs` + `src/style.rs` | 1873 + 1504 行 |
| nana-ui-sfc | `crates/nana-ui-sfc/src/lib.rs` + `src/parse.rs` | 1030 + 638 行 |
| nana-js-engine | `crates/nana-js-engine/src/lib.rs` | 1731 行 |
| nana-js-v8 | `crates/nana-js-v8/src/lib.rs` + `src/engine.rs` | 62 + 4384 行 |
| nana-ui-web-api | `crates/nana-ui-web-api/src/lib.rs`(mod audio/canvas/fetch/media/ws) | 1324 行 |

读码顺序建议:先 schema 表(看懂中间表示)→ codegen 的 `expand` 系列(看懂展开)→ style.rs 头文档(看懂构建时 CSS 的承诺)→ sfc 的 `component()`(看两条管线怎么拼)→ js-engine 的 trait 与 registry(看懂注入缝)。

## 给学习者的观察点

1. **一张表的力量**:想知道某个控件在模板里能写什么属性,答案永远在 `crates/nana-ui-view-schema/src/lib.rs` 的表里,runtime 的 setter、宏接受的属性、`.vue` 的属性都从它展开——拼错属性报错并指向行号,正是「单一事实来源」的直接收益。
2. **构建时与运行时的分界线会漂移吗**:判据是依赖表(见[模块与分层](/learn/modules-and-layers)的层级表)。链条是 runtime → nana-ui-view-macros(过程宏)→ nana-ui-view-codegen → nana-ui-css:过程宏在编译宿主上执行,CSS 解析代码不链接进应用的运行时产物;runtime 自己的依赖表里没有 nana-ui-css。
3. **注入缝先于引擎**:读 JS 面代码时,先读 `HostApiRegistry` 的注册面(renderer/web-api/startup/应用),再读 V8——合同层的注册函数签名(`api: &mut HostApiRegistry`)就是每个 host op 组的门牌。
4. **错误去哪了**:宏与 SFC 的警告都带位置(`StyleAt::Sheet` 是 CSS 文本区间、`StyleAt::Template` 是 span,`crates/nana-ui-view-codegen/src/style.rs:78`);SFC 再叠一层 `__NANA_SFC_MARKER__` 源映射。测试时故意写一个 `:hover` 规则,观察 `StyleWarning` 而不是静默。

## 延伸阅读

- [三种作者入口](/learn/authoring-paths):本章的管线在「一棵树」里的位置
- [CSS 引擎](/learn/css-engine):构建时与运行时两个时机的引擎本体
- [模块与分层](/learn/modules-and-layers):层级表与验证命令
- [端到端数据流](/learn/flows):一条更新从 JS/模板到像素
- [怎么读这份代码](/learn/reading-the-code):按图索骥的方法
