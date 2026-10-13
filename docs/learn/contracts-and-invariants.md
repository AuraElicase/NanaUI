# 合同与不变量

NanaUI 有一批**非协商合同**:它们不是风格建议,违反任何一条,框架就会出现第二套权威、第二棵树或第二个设备。这一章把合同逐条列出,每条配「为什么」与「违反后果」,并给出你可以亲自重跑的核查方法——本章写下的读数都来自当前仓库,命令原文可复现。

合同全文的另一版本见 [架构合同(参考)](/reference/architecture)。这里的差别:那一篇给改框架的人对齐口径,这一篇给学习者解释**为什么**。

## 合同是怎么被执行的

三层防线:

1. **依赖图**:分层让「不该知道的东西」在编译期就看不见(见[模块与分层](/learn/modules-and-layers))。
2. **脚本**:`scripts/check-engine-boundary.py` 扫描全工作区,守住引擎边界与 GPU 合同。
3. **评审**:本仓库的 AGENTS.md 把合同列为 Non-negotiable。

## 合同一:产品路径单向,只有一棵权威树

```text
UiWorld → ExtractedNode → UiScene → SceneWgpuPainter
```

- **是什么**:树、样式、布局、命中、焦点、IME、无障碍的权威是 `UiWorld`(runtime);绘制图权威是 `UiScene`(scene);画到屏上的只有 `SceneWgpuPainter`(nana-ui)。`LayoutBoxStore` 是投影,滚动不写回 Runtime 布局盒。
- **为什么**:任何「第二棵树」都会立刻带来双份状态同步问题——布局、命中、动画、无障碍各要同步一次,而且永远同步不齐。
- **违反后果**:`PendingHostOps` 在帧边界 commit 的语义被打破;JS 查询到的盒子与真实布局分叉。
- **核查**:`cargo metadata` 依赖图无反向边无环(读数见[模块与分层](/learn/modules-and-layers));Vue/JS 路径核查见[作者入口](/learn/authoring-paths)。

## 合同二:CSS 只有一个引擎

- **是什么**:样式表的词法、解析、选择器匹配、级联只在 `nana-ui-css` 实现。`nana-ui-core`、`nana-ui-runtime`、`nana-ui-scene`、`nana-ui` 四个 crate 不含 CSS 解析代码,只消费级联产物 Style Model。
- **为什么**:CSS 级联的语义(来源顺序、specificity、layer、important)极其复杂,两处实现必然分叉——`tools/css-parity` 整个工具存在的意义就是对齐行为。
- **违反后果**:同一份样式在 Rust 路径与 Vue 路径算出不同的计算样式。
- **核查读数(当前仓库实跑)**:
  - `nana-ui-runtime` 全量搜 `specificity|Selector|cascade|stylesheet|CssParser|tokenizer`:60 处命中全部是注释、文档、测试事件类型名(`struct Cascade`,一个测试用事件)、i18n 的 ICU `selectordinal`,以及 `view/style.rs` 对**编译产物** `Sheet`/`StylePatch` 的消费——该模块文档原话:*"The `.vue` compiler (and `css!`) parse and match a view's stylesheet … CSS is parsed and no selector is matched at run time, and this crate has [no CSS engine]"*。
  - `nana-ui-core` 搜同组词:仅 `SemanticColorRole::from_css_token_name`(`style_model.rs:341`)——把 token 名映射成语义角色,属 Style Model 词汇,不是样式表解析。
  - `nana-ui-scene`:仅一个 benchmark 文件里的注释。
  - `nana-ui`:0 命中。
  - CSS 引擎的被依赖方(依赖表实读):只有 `nana-ui-view-codegen`(构建时)与 `nana-ui-vue`(运行时)。runtime 与 nana-ui 的依赖表里没有 css。

## 合同三:输入只有一个消费方

- **是什么**:`nana-ui-input` 拥有规范输入事件、`InputEndpoint`、`InputSequencer` 与 `HostServices` 合同;宿主把原生输入降维到它,而**路由的唯一实现在 runtime**:每个 `AppContext` 的 `route_input`(`crates/nana-ui-runtime/src/framework/input.rs:397`)。
- **为什么**:输入的顺序、代际(generation)、序列号判定(丢弃乱序/重复事件)必须在一处裁决,否则焦点、拖拽、手势的组合行为无法定义。
- **违反后果**:两条路由路径对同一事件流给出不同的「按下/释放」判定。
- **核查读数(当前仓库实跑)**:
  - `route_input` 的定义只在 `crates/nana-ui-runtime/src/framework/input.rs:397`;headless 变体 `HeadlessInput::route_shaped` 在 `crates/nana-ui-runtime/src/framework/input/headless.rs:119`,同在 runtime。
  - 调用方全量清点:
    - `nana-ui`(宿主侧)在 `scene_host/host_services.rs:35,50` 构造 `InputSequencer`,把原生事件盖序列号后交给 runtime 的路由——这是「降维」,不是第二路由。
    - `nana-ui-vue` 在 `host/input_projection.rs:386` 的 `route_input` 只做 DOM/JS 事件 → 规范 payload 的翻译,内部转调 runtime 的路由(`hosted_adapter.rs:468`)。
    - `nana-ui-devtools` 的 `dispatch_input`(`agent/runtime.rs:120`)调用的是 runtime 的 `HeadlessInput::route_shaped`——消费 headless 通道,不是自建路由。
    - `nana-ui-platform` 只再导出合同类型(如剪贴板错误映射 `HostServiceError`),不路由。
  - 结论:路由实现只存在一处;其余都是降维/翻译/消费。

## 合同四:只有一个 GPU 设备与队列

- **是什么**:WGPU 是唯一后端;整个依赖图只允许一个 wgpu 主版本(当前锁定 `wgpu = "30.0.0"`)。禁止第二个 Device/Queue,禁止产品路径 CPU 回读。
- **为什么**:两个设备意味着两份提交时序,实时画面与 UI 的对齐(同一帧、同一 present)无法保证;CPU 回读会把 GPU 并行管线打穿。
- **违反后果**:画面撕裂或串帧;present 对齐失效。
- **核查**:`scripts/check-engine-boundary.py`;`nana-gpu`、`nana-frame-exchange`、`nana-ui`、`nana-ui-vue`、`nana-ui-devtools` 的公开签名里 `wgpu::*` 只在 `wgpu-interop` feature 之下出现,`nana_gpu::__framework` 仅供框架 crate 自己的源码。

## 合同五:原始窗口句柄不出现在普通控件里

- **是什么**:HWND/NSWindow 等原生句柄归 `nana-window` 持有;普通控件拿不到。系统材质失败时返回**实际应用的结果**或显式回退,不静默假装成功。
- **为什么**:控件一旦摸到句柄,就会绕过框架的绘制/命中/生命周期,产生只在某平台复现的洞。
- **违反后果**:控件直接调系统 API 画的东西不参与布局与裁剪,也无法测试。

## 合同六:逻辑状态与呈现分离

- **是什么**:`UiWorld` 是逻辑与 base 的权威;过渡中的瞬时呈现值存在 `PresentationStore` overlay。合成器动画不得把采样值写回基础样式。
- **为什么**:把 `opacity: 0→1` 动画逐帧写回树,会把瞬时值变成逻辑状态,业务读到错误数据,且稳态帧永远停不下来。
- **违反后果**:`UiWorld` 每帧变脏,compositor-steady 门禁(`perf/README.md`)失败。
- **核查**:动画完成走 deadline 而非逐帧采样;见 [GPU 与帧生命周期](/learn/gpu-frame)。

## 合同七:诊断走 nana-diagnostics,帧路径零 I/O

- **是什么**:框架诊断用 `event!`/`metric!`/`fault!` 加静态描述符(ID 只增不改)。帧路径上不允许日志 crate、`eprintln!`、格式化或 I/O;高频数据必须是 metric,不是逐帧 event。
- **为什么**:一次 `eprintln!` 在 144Hz 下就是每秒 144 次同步 I/O,直接吃掉帧预算。
- **违反后果**:帧时间尖刺;诊断 ID 漂移破坏消费方。
- 细节见 [诊断(参考)](/reference/diagnostics)。

## 合同八:没有第二套 ABI

- **是什么**:内建控件与扩展控件走同一份 `ComponentRegistry`、同一个 `register_component`。`NativeComponentRegistry` 只服务 JS host 命令,不是第二套 Runtime ABI;`PendingHostOps` 镜像、`WidgetKind`、`ComponentSupport` 都不是实例化 ABI。
- **为什么**:两套 ABI 意味着每个控件要实现两遍,特性永远不对齐。
- **违反后果**:插件控件拿不到内建控件的布局/无障碍待遇。

## 合同九:WebView 不是产品壳

- **是什么**:没有整窗 WebView 壳。应用内网页只有显式的 `runtime::BrowserView` 宿主例外(当前仅 macOS 后端);无头 `WebSurface` 页面是**宿主内容生产者**(帧交给应用 sink),不是 UI。盒模型对照工具留在 workspace 外的 `tools/css-parity-webview`,不链进产品 crate。
- **为什么**:WebView 壳会让整个 UI 退出 GPU 管线,失去保留树的全部意义。
- **违反后果**:原生内容不进 Runtime/Scene 的离屏截图;非矩形裁剪/透明/滤镜/遮挡场景由宿主隐藏处理,行为不可组合。

## 依赖方向速记

合同一~三共同指向同一条判据,你可以用它审任何 PR:

> **问「谁的权威?」**——树、样式、文本、输入、设备、窗口,每个问题都只有一个 crate 能答。其他 crate 只能提问(调用)或降维(翻译),不能再造一个答案。

## 接着读

- [模块与分层](/learn/modules-and-layers):依赖图的完整读数
- [作者入口](/learn/authoring-paths):三种输入如何汇到一棵树
- [GPU 与帧生命周期](/learn/gpu-frame):设备/帧合同在运行期的样子
