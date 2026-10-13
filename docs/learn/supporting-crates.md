---
title: 外围支撑 crate 一览
---

# 外围支撑 crate 一览

产品路径(core → runtime → scene → nana-ui)之外,仓库还有一圈支撑 crate:图表模型、诊断、SVG 栅格化、图标、打包两侧、开发期热重载与诊断台。它们不拥有树、不拥有绘制,但各自守住一条边界。这一章逐个给出:头文档的承诺、关键类型、内部结构与行号锚点,最后是仓库生态全景(examples、android、css-parity、JS 包、perf)。

## 职责与边界

| Crate | 一句话职责 | 边界(头文档原意) |
| --- | --- | --- |
| nana-ui-charts | 图表模型:ECharts 形状的 option、scales、layout、hit 数据、GPU mark 数组 | 只依赖 nana-ui-core;不碰保留树与 GPU;Chart 组件在 runtime,绘制管线在 nana-ui |
| nana-diagnostics | `event!`/`metric!`/`fault!`/`span!` 宏与静态描述符 | producer 路径零 IO、零格式化、(首事件后)零分配 |
| nana-svg-raster | CPU 侧 SVG 栅格化(resvg → pixmap) | *"Not a painter and not an SVG DOM"* |
| nana-app-icon | 默认应用标记、PNG/ICO/ICNS 编码、Windows PE 嵌入 | 分发布局(macOS .app 等)归 nana-packager |
| nana-icons-tabler | 生成的 Tabler 图标常量表 | 非手工代码;非 workspace member |
| nana-package | 打包契约的**运行时读侧** | *"Everything here reads"*;构建、签名在 nana-packager,不随应用发布 |
| nana-packager | 打包**构建侧** CLI | 已构建产物 + 资源树 + `nana-package.toml` → 可分发应用 |
| nana-ui-dev | 开发期热重载的公共部分(文件监视/debounce/安全读) | `compile_error!` 保证进不了 release/dist 构建 |
| nana-ui-devtools | 开发期诊断记录与无头 agent 会话 | *"deliberately no product-facing UI"*;产品不依赖它 |

## 依赖与邻居

层级沿用[模块与分层](/learn/modules-and-layers):

| Crate | 层级 | 依赖(normal) | 主要消费者 |
| --- | --- | --- | --- |
| nana-diagnostics | 0 | — | nana-ui-core 起的整条产品线 |
| nana-svg-raster | 0 | —(resvg 0.48.1 等) | nana-ui-web-api、nana-ui |
| nana-package | 0 | —(纯 Rust) | nana-ui(host 运行时读包) |
| nana-ui-charts | 2 | nana-ui-core + bytemuck 1.24(derive) | runtime 的 Chart 组件;nana-ui 的图表管线 |
| nana-app-icon | 3 | nana-ui-platform | nana-ui;应用 build.rs |
| nana-icons-tabler | (非 member) | nana-ui-core 的 icon 类型 | 应用按需引用 |
| nana-packager | 9(构建侧) | 链 C zstd encoder | CI/发布脚本 |
| nana-ui-dev | 9(dev-only) | notify 等 | nana-ui-sfc hot、nana-ui-vue dev |
| nana-ui-devtools | 9(dev-only) | 见 feature 嵌套 | 开发工具、agent 会话 |

值得说清的三条:

- **charts 的依赖表只有一行**:`crates/nana-ui-charts/Cargo.toml` 里 normal 依赖仅 `nana-ui-core.workspace` 与 `bytemuck 1.24`(derive,给 GPU 数组的 `Pod` 派生)——这是「图表模型独立于运行时与 GPU」在依赖层面的直接证据。
- **package/packager 的纯 Rust 分界**:workspace 注释写明运行时读侧(nana-package)用无 C/asm 后端的 blake3 与纯 Rust zstd 解码;只有构建侧 nana-packager 链 C zstd encoder。
- **dev 与 devtools 的 dev-only 性质不是口头约定**:`crates/nana-ui-dev/src/lib.rs:22` 的 `#[cfg(not(debug_assertions))] compile_error!("nana-ui-dev is development-only and must not reach a release or dist build")`——`profile.dist` 继承 release,打包必炸,故意没有逃生门。

## 关键类型与数据

| Crate | 关键类型/常量 | 定义处 |
| --- | --- | --- |
| charts | `ChartOption`(ECharts 形状,Rust 值) | `crates/nana-ui-charts/src/option.rs` |
| charts | `Length { Px(f32), Percent(f32) }` + `resolve` | `crates/nana-ui-charts/src/option.rs:18-31` |
| charts | `ChartColor { Role / Palette / Rgba }` | `crates/nana-ui-charts/src/option.rs:35` |
| charts | `GpuPoint { to, from, dist: [f32;4] }`(size_of==48 常量断言) | `crates/nana-ui-charts/src/marks.rs:27` |
| charts | `ShapeKind { Rect=0, Sector=1, Symbol=2, Needle=3 }` | `crates/nana-ui-charts/src/marks.rs:38` |
| charts | `trait LabelMeasure` | `crates/nana-ui-charts/src/layout/mod.rs:29` |
| diagnostics | `event!` / `fault!` / `metric!` / `span!` | `crates/nana-diagnostics/src/lib.rs:83/100/118/131` |
| svg-raster | `RasterizedSvg { width, height, rgba: Arc<[u8]> }` | `crates/nana-svg-raster/src/lib.rs:7` |
| package | `ResourceClass { EarlySplash=0, BootstrapUi=1, Protected=2 }` | `crates/nana-package/src/pack/mod.rs:30` |
| package | `EARLY_SPLASH_MAX_PACK_BYTES = 4MiB` | `crates/nana-package/src/pack/mod.rs:25` |
| ui-dev | `ReloadRequest { Full, Css, Template }` | `crates/nana-ui-dev/src/lib.rs:58` |
| ui-dev | `MAX_ARTIFACT_BYTES = 64MiB` | `crates/nana-ui-dev/src/lib.rs:54` |
| devtools | `DiagnosticKind`(12 种) | `crates/nana-ui-devtools/src/lib.rs:32` |
| devtools | `DEFAULT_EVENT_CAPACITY = 2048` | `crates/nana-ui-devtools/src/lib.rs` |

## 内部结构

### nana-ui-charts:CPU 布局,GPU 消费

`crates/nana-ui-charts/src/lib.rs`(27 行)只做模块声明与 re-export(option/marks/layout/hit/scale/smooth/stack/theme/transition/sample)。头文档一句话讲清分工:CPU 把 `ChartOption` 布局成文本、hit 数据与 `ChartMarks`(图表 shader 读的数组);不碰保留树与 GPU;`Chart` 组件在 runtime,scene 携带 marks,nana-ui 绘制。

- **option**(1424 行):形状跟随 ECharts——grid/xAxis/series/tooltip/legend/dataZoom 都有对应字段,但值是 Rust 类型:`Length` 的 `Px`/`Percent` 经 `resolve` 按容器尺寸求值;`ChartColor` 可以是语义角色(`SemanticColorRole`)、调色板序号或字面 `[f32;4]`;数值标签是 `ValueFormatter`(`Arc<dyn Fn(f64) -> Arc<str> + Send + Sync>`,`crates/nana-ui-charts/src/option.rs:45`),时间标签是 `TimeFormatter`(`:71`)。头文档的权责句:*"Applications own the values and every label; the chart owns geometry, motion and interaction."*
- **marks**(307 行):头文档直接声明它是 WGSL 合同——*"layouts here **are** the WGSL contract (scene_paint/shader/chart.wgsl in nana-ui); a field moves in both places or neither"*。每个 mark 带 `to`/`from`,顶点着色器按 eased `MarkTransition` 进度插值,动画零 CPU 成本。`ShapeKind` 四种各自定义 to/from/extra 的字段布局(注释逐条对应);`SymbolShape` 有 Circle/Rect/RoundRect/Triangle/Diamond/Pin(`:59`);标志位 `SHAPE_CLIP_TO_PLOT = 1<<8`、`SHAPE_EMPHASIS_GROW = 1<<9`、`SHAPE_REVEAL = 1<<10`(`:70-74`)。
- **layout**(`layout/mod.rs`,276 行):头文档约定 *hover 不是输入*——布局只依赖 option、盒子尺寸、theme、`LabelMeasure` 与 `ChartViewState`,悬停答案由**已完成的**布局 `ChartLayout::hover_at` 回答,移动指针从不触发重排版。宿主文本引擎经 `trait LabelMeasure { fn measure(&self, text: &str, size: f32) -> [f32; 2] }`(`crates/nana-ui-charts/src/layout/mod.rs:29`)注入测量;测试用固定 advance 的 `ApproximateMeasure`(`:36`);`ChartViewState { hidden, zoom }`(`:51`)承载图例开关与缩放。子模块 builder/cartesian/legend/polar 分坐标系。
- **hit**:悬停几何(`ChartHover`/`PointerGeometry`/`TooltipContent`/`TooltipRow`,re-export 于 `crates/nana-ui-charts/src/lib.rs:20`)。
- **其余模块**(声明于 `crates/nana-ui-charts/src/lib.rs:9-18`):scale、smooth、stack、transition、sample——本章没有逐个展开,读码时从 `lib.rs` 的 re-export 清单入手最省力;theme 导出 `ChartTheme`(`:27`)。

### nana-diagnostics:四个宏,一条通道

`crates/nana-diagnostics/src/lib.rs`(135 行)头文档是一张架构图:producer 线程上,`event!`/`fault!` 进 thread-local SPSC 环形缓冲(满即丢),`metric!`/`span!` 写静态原子(轮询消费、不通知,`fault!` 额外唤醒 worker 保险存);worker 线程低优先级,持最近约 60 秒的 flight recorder,攒批一次顺序写 `.nlog`。producer 路径的硬承诺:无文件 IO、无 fsync、不等 worker、无反压、无全局锁、无格式化、首事件后无分配(`fault!` 的 message 是唯一一次分配);实例未安装时每次调用退化为一次 relaxed atomic load。四个宏的用法直接写在 lib.rs:`event!(MODEL_LOADED, bytes=1024u64)`(`:83`,字段名只在 debug 构建对描述符检查)、`fault!(DEVICE_LOST, reason=1u64; "device lost: {message}")`(`:100`,message 仅在记录时格式化)、`metric!(TRACK_NS, Duration)`(`:118`,counter/gauge/histogram 通吃整数、bool、Duration)、`span!(LAYOUT_NS) -> SpanGuard`(`:131`)计时作用域。

框架自己的描述符在 `crates/nana-diagnostics/src/framework.rs`(635 行),domains 0x0001..=0x0009。第 2 行就是契约:*"IDs are part of the `.nlog` contract: never renumber, only append"*——注释里还留着退役 ID(22/25/27-30/32/38/39)占位的例子。runtime 域(`framework.rs:20` 起)的骨架:`FRAME_CPU_NS`(直方图,`framework.rs:26`)、`FLUSHES`(`:28`)、`FLUSH_DID_NOT_SETTLE`(`:32`)、`FLUSH_FAILED`(`:35`),九个阶段直方图 `STAGE_INPUT_NS`..`STAGE_EXTRACT_NS`(`:37-45`:input/reconcile/style/text_shape/layout/hit_test/accessibility/animation/extract),输入路由计数 `INPUT_EVENTS`/`INPUT_COALESCED`/`INPUT_ROUTE_NS`/`INPUT_HIT_TESTS` 等(`:50-63`)。应用自定义事件从 `Domain::APPLICATION_MIN` 起。导出面分五块:schema(描述符类型)、runtime(install/global/snapshot)、export(to_json_lines/to_text)、package、crash(install_panic_hook)。

四个宏的通道与语义速查:

| 宏 | 通道 | 特点 |
| --- | --- | --- |
| `event!` | thread-local SPSC 环形(满即丢) | 低频事实事件,附字段 |
| `fault!` | 同上,并唤醒 worker 保险存 | 设备丢失级故障;message 仅在记录时格式化 |
| `metric!` | 静态原子(counter/gauge/histogram) | 高频数据,轮询消费不通知——**高频永远是 metric,不是每帧一个 event** |
| `span!` | 静态原子 + `SpanGuard` | 作用域计时,drop 时落样本 |

### nana-svg-raster:四个入口,一种产物

`crates/nana-ui-svg-raster/src/lib.rs`(243 行)头文档:*"CPU SVG raster (resvg → pixmap). Not a painter and not an SVG DOM."* 产物统一是 `RasterizedSvg { width, height, rgba: Arc<[u8]> }`(`:7`)。四个入口对应四类调用方:

| 函数 | 行 | 调用方与语义 |
| --- | --- | --- |
| `rasterize_stretch` | `crates/nana-svg-raster/src/lib.rs:21` | Vue 通用 `<svg>`:独立拉伸到目标尺寸;`SvgFont { bytes, family }`(`:15`)供 SVG `<text>` 嵌入字体 |
| `rasterize_white_mask` | `crates/nana-svg-raster/src/lib.rs:42` | 图标 atlas:contain 进方形,currentColor 替换为白 |
| `rasterize_document` | `crates/nana-svg-raster/src/lib.rs:58` | Canvas Image:按文档尺寸、直接 alpha |
| `rasterize_document_capped` / `_with_font` | `crates/nana-svg-raster/src/lib.rs:71-77` | URL SVG 解码,封顶 max_edge |

### nana-app-icon:默认标记 + 三平台编码

`crates/nana-app-icon/src/lib.rs`(140 行)头文档把边界划在编码与嵌入:PNG/ICO/ICNS 编码、Windows PE 嵌入在这里;分发布局(macOS .app、Windows/Linux 目录)是 nana-packager 的事。Nana 剪影是 fallback 身份——应用运行时注册自己的 `nana_ui_platform::WindowIcon`,或在 build.rs 里嵌 `.ico`。两个 feature:`embed`(→ `embed_windows`/`embed_windows_from`)与 `runtime`(→ `runtime_api`:`with_system_grid` 把图标收进 macOS Dock 的 828/1024 网格(`lib.rs:32`,声明 `exact_pixels` 则原样)、`window_icon_from_png`、`default_window_icon`(`:65`,OnceLock 缓存 256px)、`resolved_application_icon`)。workspace 对它的引用带 `default-features = false`。`crates/nana-app-icon/src/mark.rs:8` 的 `rasterize` 在预乘空间缩放 `assets/nana.png` 避免暗边;90% 图框,可见板约占画布 83%。

### nana-icons-tabler:生成物,链接器裁剪

`crates/nana-icons-tabler/src/lib.rs` 是一个 383983 行的生成文件,头注释:*"@generated by scripts/generate_tabler_catalog.py from @tabler/icons 3.46.0 (MIT). Do not edit by hand."* 每个图标是四件套:`A_B_PATH_N: &[IconPathCommand]`(MoveTo/LineTo/CubicTo)、`A_B_SHAPES: &[IconShape]`、`A_B_SVG: &str`(原样 24×24 stroke SVG)与 `pub static A_B_ICON: IconData { name, shapes, svg }`;类型来自 `nana_ui_core::icon::{Icon, IconData, IconPathCommand, IconShape}`。体积不构成负担的原因写在头注释里:消费者引用根 `Icon` 常量,链接器丢弃每个未被引用的图标,只有用到的几何进二进制。它被根 Cargo.toml `exclude`(不是 workspace member),但经 `workspace.dependencies` 可正常引用;含 `fill="currentColor"` 子形状的图标由 resvg(svg-raster)精确栅格化。

### nana-package 与 nana-packager:同一契约的读侧与写侧

Issue #226 的打包契约拆成两端。**读侧** `crates/nana-package/src/lib.rs`(102 行)头文档:*"Everything here reads. Building packs, generating keys and signing is the build-side nana-packager, which never ships inside an application."* 包形状在 `crates/nana-package/src/pack/mod.rs`(223 行):`.nrpack` = header + data region(每 entry 一段 extents)+ TOC;每个 entry 切固定大小明文块,**每块独立压缩密封**——读一个资源只碰它自己的块,改一个资源只重写它自己的 extent 与 TOC(注释注明形状参考 SteamPipe deltas)。`ResourceClass`(`pack/mod.rs:30`)是有序的:EarlySplash=0 / BootstrapUi=1 / Protected=2,一个 pack 只能依赖同层或更早层的资源;启动首屏预算 `EARLY_SPLASH_MAX_PACK_BYTES = 4MiB`(`:25`)。lib.rs 还定义运行时自检协议:`SELF_CHECK_ENV = "NANA_PACKAGE_VALIDATE"`(`crates/nana-package/src/lib.rs:42`)、`SELF_CHECK_PREFIX`(`:44`)、`valid_logical_path`(`:50`,包内路径的合法性:/分隔、非空段、无 `\ / :` 与控制字符、不超 `MAX_KEY_LEN`)。

**写侧** `crates/nana-packager/src/main.rs`(386 行)是 CLI,六个子命令(`crates/nana-packager/src/main.rs:32-38`):`package`(`--config nana-package.toml --exe PATH --out DIR`,可选 `--target/--baseline/--cache/--profile dist/--content-key-file/--signing-key-file/--allow-in-tree-key`)、`validate APP_ROOT`(`--trust-key ed25519:HEX`,可 `--run`、`--tamper-suite`)、`keygen content|publisher`、`inspect`、`delta OLD NEW`、`macos-app`。库侧模块(cache/config/delta/layout/macos/pack_build/package/plan/secrets/sign/steam/util/validate)把「已构建可执行文件 + 资源树 + 配置」变成可分发应用:打包、平台布局、manifest、Steam depot 元数据、验证报告。端到端验收由 examples/package-fixture 充当被试(*"packaged, launched from a foreign directory, and tampered with"*,其 Cargo.toml description)。

### nana-ui-dev:热重载的公共底座

`crates/nana-ui-dev/src/lib.rs`(209 行)只做各 tier 相同的部分:文件系统监视、抗「编辑器 write-truncate-rename」的 debounce、拒绝半写文件的 jailed read;**reload 做什么**由 tier 决定(nana_ui_vue::dev 或应用自身)。reload 请求分三档(`ReloadRequest`,`crates/nana-ui-dev/src/lib.rs:58`):`Full` 重读 artifact 重挂树——窗口、位置、GPU device/surface 保留,页面状态不保留(浏览器刷新语义);`Css { path }` 原位替换一个 keyed stylesheet——不建/毁节点,node id、focus、scroll、动画全保留;`Template { path }` 配合 SFC 热文本。失败全部可恢复:`DevError::Unreadable/Empty/NotUtf8/NonUtf8Path`(`:92`,Empty 即半写文件被 jailed read 拒下)。`MAX_ARTIFACT_BYTES = 64MiB`(`:54`)远大于 core 的 `MAX_LOCAL_URL_BYTES`(8MiB),因为未压缩的 dev bundle 带内联 source map。`crates/nana-ui-dev/src/watch.rs:25` 的 `Debouncer` 按路径记最新事件、整批只在静默 `quiet` 之后释放——防止 bundler 一次 emit 拆成多次 reload;`take_ready` 返回排序路径,CSS 快路径按这个顺序应用。feature:`vue`(`watch_and_reload`)、`templates`(`watch_templates`,驱动 nana-ui-sfc 的 hot 模式,见[作者工具链](/learn/toolchain))。

### nana-ui-devtools:有界记录 + 无头 agent

`crates/nana-ui-devtools/src/lib.rs`(402 行)面向 Vue/JS 与 hosted 渲染栈的开发期诊断:*"stores bounded structured records, deliberately no product-facing UI"*。`DiagnosticKind` 十二种(`crates/nana-ui-devtools/src/lib.rs:32`:JsException/UnhandledPromiseRejection/VueWarning/VueError/HostCall/ResourceLifecycle/WindowLifecycle/Frame/DeviceLost/RenderError/Inspector);`FrameStatistics` 累计 build/draw/present 微秒与丢帧;环形容量 `DEFAULT_EVENT_CAPACITY = 2048`。`DevtoolsSession` 是可克隆 recorder,反过来对接 nana-js-engine 的两个观察点:`js_sink()` 给引擎的 `JsDiagnosticSink`、`host_call_observer()` 给 `HostApiRegistry` 的调用观察器——**只记方法名、状态、时序,绝不记参数与结果**。feature 是嵌套的:`offscreen`(绘制 + CPU readback)⊂ `runtime-agent`(加 JSON 协议、`agent::AgentSession` trait、不拉 JS 引擎的纯 Rust 会话与 nana-runtime-agent bin)⊂ `agent`(加 Vue/JS 会话)⊂ `agent-bin`(加 V8 与 nana-agent-session)。协议放在 runtime-agent 层,纯 Rust 产品可以只说协议、不带 Vue renderer 与 V8。会话本体在 `crates/nana-ui-devtools/src/agent/mod.rs`(313 行):无头 agent = offscreen 像素 + 进程内 pointer/keyboard,头文档注明 *"CPU readback stays here. Product run_runtime present never uses this path."*;`runtime::RuntimeAgentSession` 驱动纯 Runtime 文档,`vue::VueAgentSession` 需 agent feature,两者都 impl `session::AgentSession`——命令分发与 stdio 循环只写一次,协议类型(`AgentCommand`/`AgentReply`/`PixelDiff`/`PixelStats`/`PointerGesture`/`KeyStroke`/`InspectDump`/`LayoutNodeDump`/`HitDump`/`A11yFilter`/`Target`)不含 Vue 类型。

## 协作与数据流

| 支撑 crate | 上游 | 下游(数据流向) |
| --- | --- | --- |
| nana-ui-charts | 应用的 ChartOption + theme | runtime 的 `Chart` 组件(AGENTS.md 定义的消费方)→ charts 布局产出 marks/文本/hit 进 scene → nana-ui 的 `chart.wgsl` 绘制 |
| nana-diagnostics | 全仓 `event!/metric!/fault!/span!` 调用点 | worker 写 `.nlog`;export 转 JSON lines/text |
| nana-svg-raster | Vue `<svg>`、Canvas Image、图标源 | `RasterizedSvg` 位图给纹理上传 |
| nana-app-icon | `assets/nana.png` | 应用窗口图标 / PE 嵌入资源;布局由 packager |
| nana-icons-tabler | 生成脚本 + Tabler 上游 | `IconData` 常量,链接器裁剪 |
| nana-package | nana-packager 产出的 `.nrpack` | nana-ui host 运行时读取(含启动自检) |
| nana-ui-dev | 文件系统事件 | `ReloadRequest` → tier 的 reload 实现;SFC hot 的 `watch_templates` |
| nana-ui-devtools | js-engine 的诊断 sink 与 host-call 观察器 | 有界记录 + agent 协议(stdio/像素 diff) |

## 代码地图

| Crate | 入口文件 | 内部模块 |
| --- | --- | --- |
| nana-ui-charts | `crates/nana-ui-charts/src/lib.rs` | option/marks/layout(hit 由 builder/cartesian/legend/polar 支撑)/scale/smooth/stack/theme/transition/sample |
| nana-diagnostics | `crates/nana-diagnostics/src/lib.rs` | framework(静态描述符)、schema/runtime/export/package/crash |
| nana-svg-raster | `crates/nana-svg-raster/src/lib.rs` | 单文件 |
| nana-app-icon | `crates/nana-app-icon/src/lib.rs` | encode、mark;features: embed / runtime |
| nana-icons-tabler | `crates/nana-icons-tabler/src/lib.rs` | data(生成) |
| nana-package | `crates/nana-package/src/lib.rs` | hash/identity/keys/manifest/pack |
| nana-packager | `crates/nana-packager/src/main.rs` + `src/lib.rs` | cache/config/delta/layout/macos/pack_build/package/plan/secrets/sign/steam/util/validate |
| nana-ui-dev | `crates/nana-ui-dev/src/lib.rs` | watch(Debouncer/DevWatcher);features: vue / templates |
| nana-ui-devtools | `crates/nana-ui-devtools/src/lib.rs` | agent(runtime/vue/session);features: offscreen ⊂ runtime-agent ⊂ agent ⊂ agent-bin |

## 怎么验证这些边界

本章每条边界都有只读证据可查,不需要跑任何构建:

| 断言 | 查证处 |
| --- | --- |
| charts 只依赖 core | `crates/nana-ui-charts/Cargo.toml` 的 `[dependencies]` 只有 nana-ui-core.workspace 与 bytemuck |
| dev crate 进不了发布构建 | `crates/nana-ui-dev/src/lib.rs:22` 的 `compile_error!`;`profile.dist` 继承 release |
| icons-tabler 不在 workspace | 根 `Cargo.toml` 的 `exclude = ["tools/css-parity-webview", "crates/nana-icons-tabler"]`,但它在 `workspace.dependencies` 里可引用 |
| 诊断 ID 只增不改 | `crates/nana-diagnostics/src/framework.rs:2` 头注释 + 退役 ID 占位注释 |
| 打包读侧/写侧互指 | 两个 crate 头文档各写明对方:package 说 *"Building packs … is the build-side nana-packager"*,packager 说自己是 build-side |
| 图表绘制不在 charts | `crates/nana-ui-charts/src/marks.rs` 头文档:WGSL 在 nana-ui 的 `scene_paint/shader/chart.wgsl` |
| js-v8 重依赖是测试用 | `crates/nana-js-v8/Cargo.toml` 的 `[dev-dependencies]` 段 |

## 仓库生态一览

工作区 members 除 crates 外还包括 8 个 examples、platform/android 与 tools/css-parity;`crates/nana-icons-tabler` 与 workspace 外的 `tools/css-parity-webview` 被 exclude。

**examples/(8 个)**,描述取自各自 Cargo.toml:

| Example | 定位 |
| --- | --- |
| component-gallery | 控件画廊;另带 ui-benchmark/ui-snapshots/ui-migration-ab/ui-host-ab 四个 bin(`examples/component-gallery/Cargo.toml:60-75`) |
| vue-v8 | "V8 Vue runtime-core probe" |
| vue-counter | "Vue Counter/Todo JS probe + optional Runtime/UiScene window" |
| vue-hosted-acceptance | "Windowed V8 acceptance app for pure Vue and Vue/Runtime composition" |
| reactive-sfc | "Views written as .vue files with Rust scripts, compiled by nana-ui-sfc in build.rs" |
| native-rhi-probe | 原生后端探针(Cargo.toml 无 description,按名字直译) |
| runtime-host-fixture | Runtime 宿主夹具(Cargo.toml 无 description,按名字直译) |
| package-fixture | "Headless subject of the Issue #226 final-artifact validation: packaged, launched from a foreign directory, and tampered with" |

**platform/android**:实验性 ARM64 host,非产品目标。头文档:GameActivity + GameTextInput InputConnection + ANativeWindow Surface + V8(wgpu 30/Vulkan);「control slot 是 NanaUI Runtime + UiScene + SceneWgpuPainter,不是第二个产品绘制内核」。12 个模块(lib/engine/gpu/runtime/shell/control_slot/chrome_fill/slot_ax/slot_ime/slot_input/slot_paint/slot_runtime),`android_main` 入口,桌面构建暴露 `smoke_engine_only`,能力开关用 `nana_ui_platform::PlatformCapabilities::android_mvp()`。

**tools/css-parity**:CSS 布局一致性测试 harness(workspace member)。Nana 侧经 `nana_ui_vue::measure_layout` 适配 `RuntimeLayoutEngine::layout_style_tree` 共享算法;参照是 fixture 内嵌的 expected 盒;容差 `DEFAULT_TOLERANCE_PX = 2.0`;`CaseStatus::Ignore` 标注 P0-x 缺口(`#[ignore]`)。本机 WKWebView 对照在 workspace 之外的 tools/css-parity-webview,不得链进 nana-ui,也不进默认依赖或产品运行时。

**packages/**:两个 JS 包(不进 Rust workspace)。nanavue-runtime 是 Vue Custom Renderer 底座(src:events/nodes/props/styles/windowContext/transitionContract.js;tests:dom-contract/host-ops/patch-prop/teleport-contract 与 vue stub);nanavue-components 是 60+ 个 `Nana*.js` 控件库(NanaButton、NanaDock、NanaWorkspace、NanaVirtualList、NanaTerminal……)加 nana-controls.css 与 virtual-window.js。

**perf/**:性能 harness,共享 Scenario schema + 各框架薄 runner(runners:nana/iced/gpui;micro 与 baselines 目前 reserved;reports gitignored)。README 明确 #8 DoD 是 Nana 自身 work-counter/catalog/hotspot 与 CI fail-closed,跨框架同批数字只算 #12 的 observation、不作 pass/fail。专用场景组:

| 场景组 | 内容 |
| --- | --- |
| nana_gpu_scale_ids | 4 个 GpuScene/UiOnly 规模场景(不在 CI 的 harness_ids 里) |
| windows-composition-steady | Windows + DX12 真机,六个 invariant 全部要求 eq 0 |
| nana_text_ids | #98 的三类文本门禁 + 亚像素平移 + constraint-only |

## 延伸阅读

- [GPU 帧与绘制](/learn/gpu-frame):charts 的 marks 如何被 chart.wgsl 消费
- [作者工具链](/learn/toolchain):nana-ui-dev 的 `watch_templates` 服务的那条 SFC 管线
- [模块与分层](/learn/modules-and-layers):层级总表与 dev 依赖例外
- [窗口与平台](/learn/window-platform):app-icon 产物的消费端
- [诊断参考](/reference/diagnostics):`event!/metric!/fault!` 的完整用法与 ID 纪律
- [证据清单](/learn/evidence-checklist):如何验证这些边界真的成立
