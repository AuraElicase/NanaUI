---
title: 作者工具链
---

# 作者工具链

开发者写下的 `view!`、`css!`、`stylesheet!` 宏以及 `.vue` 单文件组件，在**应用编译期**就会被编译为 `nana-ui-runtime` 的声明式视图函数调用与静态 Style Model 补丁数据；而 JavaScript / Vue 运行时侧则由「一份引擎抽象合同 + 一个 V8 实现 + 一组宿主 Web API」构成。本章将两条构建期编译管线与 JS 引擎层逐一拆开，看清模板、样式与脚本在每个 crate 中的转换过程。

## 职责与边界

| Crate | 一句话职责 | 明确不做的事 |
| --- | --- | --- |
| `nana-ui-view-schema` | 内置控件的唯一描述表（`for_each_control!` 中间表示） | 零外部依赖，不含任何代码生成逻辑 |
| `nana-ui-view-macros` | `view!` / `css!` / `stylesheet!` 与 `#[derive(Store)]` 过程宏入口 | 只做过程宏转发，核心展开逻辑交给 `view-codegen` |
| `nana-ui-view-codegen` | 将模板元素树展开为 Runtime 视图函数；在构建时调用 `nana-ui-css` 编译样式 | 不引入任何新的运行时概念 |
| `nana-ui-sfc` | `.vue` 单文件组件（`<script setup lang="rust">`）构建期编译器 | 不进入应用运行时；构建期输出标准 Rust 源码与源映射 |
| `nana-js-engine` | `JsEngine` trait 与 `HostApiRegistry` 宿主桥接合同（兼做无 V8 测试注入缝） | 不做绘制，不直接触碰保留树 |
| `nana-js-v8` | 基于 V8（`152.2.0`）的 `JsEngine` 生产实现 | 未启用 `engine` feature 时全部方法为轻量 stub |
| `nana-ui-web-api` | JS 侧 `window` / `document` / `fetch` / 定时器 / Web Audio PCM 等宿主 API | 不是浏览器 WebView，也不是第二条绘制管线 |

两条编译管线共同遵守一条铁律：**运行时收到的是编译好的数据，而非待解析的 CSS 源码**（`crates/nana-ui-runtime/src/view/style.rs:1-27`）。

## 依赖与邻居

| Crate | 层级 | 工作区内普通依赖（normal） | 边界说明 |
| --- | --- | --- | --- |
| `nana-ui-view-schema` | 0 | — | 零依赖纯宏表，同时喂给 `runtime` 与 `view-codegen` |
| `nana-js-engine` | 0 | — | 零依赖引擎合同 |
| `nana-ui-css` | 2 | `nana-ui-core` | 唯一 CSS 引擎，被构建期 `view-codegen` 与运行期 `nana-ui-vue` 共用 |
| `nana-ui-view-codegen` | 3 | `nana-ui-core`, `nana-ui-css`, `nana-ui-view-schema` | 在宿主编译期运行，将 CSS 规则编译为静态 `StylePatch` |
| `nana-js-v8` | 3 | `nana-js-engine` | 仅在开启 `engine` feature 时链接 `v8`（其重型依赖均在 `[dev-dependencies]` 中） |
| `nana-ui-web-api` | 4 | `nana-js-engine`, `nana-ui-core`, `nana-ui-platform`, `nana-text`, `nana-svg-raster` | 提供受控的 Web API 兼容子集 |
| `nana-ui-view-macros` | 4 | `nana-ui-view-codegen` | `proc-macro` crate，经 `nana-ui-runtime`（`view-macro` feature）再导出 |
| `nana-ui-sfc` | 4 | `nana-ui-view-codegen` | 在应用的 `build.rs` 中运行，生成代码通过路径字符串引用 `nana_ui::runtime` |

## 关键类型与数据

| 类型 / 常量 | 所在文件与行号 | 作用说明 |
| --- | --- | --- |
| `for_each_control!` | `crates/nana-ui-view-schema/src/lib.rs:49` | 26 个内置声明式控件的标签、构造函数、字段写入策略与事件表 |
| `Control` / `CONTROLS` | `crates/nana-ui-view-codegen/src/lib.rs:18` / `:53-77` | 由 `for_each_control!` 展开的静态控件元数据表 |
| `Node` / `Element` / `Attr` | `crates/nana-ui-view-codegen/src/lib.rs:333-405` | 模板语法树的中间表示 |
| `CompiledStyles` / `Rank` | `crates/nana-ui-view-codegen/src/style.rs:978` / `:49` | 构建时样式编译产物与级联优先级三元组 `(important, specificity, source_order)` |
| `StyleWarning` | `crates/nana-ui-view-codegen/src/style.rs:87` | 构建时无法编入的子集外选择器/属性警告（带精确行列位置，绝不静默丢弃） |
| `Component` / `Compiler` | `crates/nana-ui-sfc/src/parse.rs:23` / `lib.rs:115` | `.vue` 文件解析出的四大块（name / props / script / template / style）与构建编译器 |
| `JsEngine` / `HostApiRegistry` | `crates/nana-js-engine/src/lib.rs:1254` / `:1068` | JS 引擎抽象接口与同步/异步 Host Ops 注册表 |
| `HostValue` / `RuntimeArtifact` | `crates/nana-js-engine/src/lib.rs:287` / `:486` | JS ↔ Rust 跨界值 ABI 与装载进引擎的代码包 |
| `WebApiState` | `crates/nana-ui-web-api/src/lib.rs:75` | 宿主 Web API 的可变状态（`localStorage`、`rAF`、定时器、`FetchRuntime`、`SocketRuntime`） |

## 内部结构

### 1. 管线一：Rust 过程宏链（`view!` / `css!` / `stylesheet!`）

在 Rust 路径中，`view!` 宏与声明式视图函数是一等等价写法，展开为同一套构建与挂载调用：

```rust
use nana_ui::runtime::view;

view! {
    <Column gap=8>
        <Button @activate={save}>"确定"</Button>
    </Column>
}
```

四个过程宏定义在 `crates/nana-ui-view-macros/src/lib.rs`，由 `nana-ui-runtime` 在 `view-macro` feature 下再导出：

| 宏名称 | 入口位置 | 编译期展开行为 |
| --- | --- | --- |
| `view!` | `view-macros/src/lib.rs:17` | 解析 `<Tag>` 模板树并交给 `view-codegen`，展开为 `column` / `text` / `each` / `when` 等视图函数调用 |
| `#[derive(Store)]` | `view-macros/src/lib.rs:24` | 为结构体生成 `<Name>StoreFields` trait，为每个具名字段提供路径追踪方法 |
| `css!` | `view-macros/src/lib.rs:41` | 接收单元素 CSS 声明块，构建时调用 `nana-ui-css` 解析为 `StylePatch` 供 `.css(...)` 使用 |
| `stylesheet!` | `view-macros/src/lib.rs:76` | 将 `mod styles; .card { … }` 编译为包含静态 `Sheet` 与同名 `view::Class` 常量的模块 |

#### 单一事实来源：`nana-ui-view-schema`

`crates/nana-ui-view-schema/src/lib.rs:49` 的 `for_each_control!` 宏在一处集中定义了内置控件的模板标签、构造函数、字段写入方式（`write`）与事件：

- `nana-ui-runtime` 将这张表展开为每个控件的类型化 setter、`.model(...)` 与事件绑定方法；
- `nana-ui-view-codegen` 将同一张表展开为 `<Tag>` 模板编译期校验表——因此拼写错误的标签名或属性名在编译期就会在对应行号直接报错，而在表里新增一个字段则两边同时生效。
- 字段写入策略 `write`（`lib.rs:23-40`）精确控制值如何落入组件：`set`（普通字段直接赋值）、`text_state`（编辑文本仅在内容实际不同时替换，保护输入光标位置）、`assign`（数值输入框赋值并将光标移至末尾）、`clamp`（滑块范围钳制）、`span_low` / `span_high` / `span`（双滑块区间钳制，推着另一端移动而不允许交叉）、`builder`（调用同名构建器方法以维护派生分量，如 `ColorField` 的 HSV）、`tab`（切换选中页签并同步焦点）、`resource`（绑定宿主纹理槽）。

#### 构建时样式编译：`view-codegen/src/style.rs`

`crates/nana-ui-view-codegen/src/style.rs` 提供 `compile_inline`（`css!`）、`compile_styles`（`<style>`）与 `compile_stylesheet`（`stylesheet!`）三个入口。它在编译期调用 `nana-ui-css` 解析样式表，将类选择器（如 `.card`、`.card.active`）按级联优先级 `Rank` 编译为 `StylePatch`；将 `transition` 编译为元素隐式过渡动画；将 `@container` 块内的类规则编译为带容器轴与区间桶的数据。对于不支持的复杂选择器、`:hover`、`@media` 或未知属性，统一生成带源码位置的 `StyleWarning`，绝不静默吞掉。

### 2. 管线二：`.vue` 单文件组件编译器（`nana-ui-sfc`）

`nana-ui-sfc` 在应用的 `build.rs` 中将 `<script setup lang="rust">` 的 `.vue` 文件编译为标准 Rust 视图函数（例如 `TodoList.vue` 生成 `pub fn todo_list(...) -> impl IntoView`）：

1. **三块拆解（`parse.rs:23`）**：解析出 `<script setup>`、`<template>` 与 `<style>`，并记录行列偏移以还原错误位置；
2. **全组件静态信号分析与常量折叠（`lib.rs:413`）**：由于编译器能看见整段脚本，它会将从未被写入且未逃逸的 `signal(v)` 自动折叠为 `constant(v)`（只读常量的属性绑定在建节点时直接赋值一次，不创建运行期 `Effect`）；检测出 `computed` 循环依赖直接报编译错误；将可见的静态依赖列表包装为 `__checked`（debug 下核对实际读取是否越界，见 `runtime.reactive.static_deps_mismatch`）；
3. **产物输出（`$OUT_DIR`）**：生成格式化好的 `nana_views.rs`、依赖报告 `nana_views.deps.md` 以及源映射 `nana_views.map.json`（遵循 `nana-sfc-source-map/1`）。开启 `.hot(true)` 时还会将纯静态文本提入可热替换表 `__NANA_HOT`，配合 `nana-ui-dev` 的 `watch_templates` 实现免重启换字。

### 3. JS 引擎层：`JsEngine` 合同、`V8Engine` 与 `nana-ui-web-api`

- **抽象合同与测试注入缝（`nana-js-engine`）**：`pub trait JsEngine`（`crates/nana-js-engine/src/lib.rs:1254-1345`）定义了代码装载（`initialize`）、宿主 API 注册（`register_host_api`）、函数调用（`resolve_function` / `invoke`）、微任务排空（`run_microtasks`）与多窗口多 Realm 隔离（`create_realm` / `dispose_realm`）。所有跨界操作均注册在 `HostApiRegistry`（`:1068`）上——这使 `nana-ui-vue` 的单元测试可以直接构造 `HostApiRegistry::new()` 测试全部 DOM host ops，无需启动真实的 V8。
- **V8 生产实现（`nana-js-v8`）**：在 `engine` feature 下链接 `v8 152.2.0`（`crates/nana-js-v8/src/engine.rs`，约 4,380 行），将每个 Realm 的宿主上下文存入 V8 context slot，并将未捕获异常与 Promise rejection 转发为结构化的 `JsDiagnosticEvent`。
- **受控 Web API 子集（`nana-ui-web-api`）**：将构建期生成的 `WEB_API_SHIM_JS` 与应用 bundle 拼合为 `RuntimeArtifact`（`crates/nana-ui-web-api/src/lib.rs:65`），通过 `WebApiState`（`:75`）提供 `window` / `document` 子集、`localStorage`、`rAF`、受白名单管控的流式 `fetch`、`WebSocket`、Canvas 2D（基于 `nana-text` + `swash` + `nana-svg-raster`）与 Web Audio PCM 子集。

## 协作与数据流

```text
管线一  view! / css! / stylesheet! ──view-macros──▶ view-codegen（展开 + 构建时 CSS）──▶ Runtime 视图函数 + StylePatch
管线二  .vue 文件 ──nana-ui-sfc──▶ 信号静态分析折叠 + view-codegen 展开 ──▶ $OUT_DIR/nana_views.rs
JS 面   应用 bundle + Web API shim ──▶ V8Engine（nana-js-v8）◀──HostApiRegistry（nana-js-engine）──▶ PendingHostOps ──▶ UiWorld
```

## 代码地图

| Crate | 核心入口文件 | 规模与定位 |
| --- | --- | --- |
| `nana-ui-view-schema` | `crates/nana-ui-view-schema/src/lib.rs` | 236 行：`for_each_control!` 唯一内置控件表 |
| `nana-ui-view-macros` | `crates/nana-ui-view-macros/src/lib.rs` | 685 行：`view!`、`css!`、`stylesheet!`、`Store` 过程宏入口 |
| `nana-ui-view-codegen` | `crates/nana-ui-view-codegen/src/{lib,style}.rs` | 1,873 + 1,504 行：模板展开器与构建时 CSS 编译器 |
| `nana-ui-sfc` | `crates/nana-ui-sfc/src/{lib,parse}.rs` | 1,030 + 638 行：`.vue` 编译器与静态信号依赖分析 |
| `nana-js-engine` | `crates/nana-js-engine/src/lib.rs` | 1,731 行：`JsEngine` trait、`HostApiRegistry` 与 `HostValue` |
| `nana-js-v8` | `crates/nana-js-v8/src/{lib,engine}.rs` | 62 + 4,384 行：V8 Isolate / Realm 宿主实现 |
| `nana-ui-web-api` | `crates/nana-ui-web-api/src/lib.rs` | 1,324 行：Web API 兼容子集与 `WebApiState` |

## 延伸阅读

- [三种作者入口](/learn/authoring-paths)：三条编写路径如何在 `UiMutation` / `UiWorld` 汇合
- [CSS 引擎](/learn/css-engine)：构建时与运行时共用的 `nana-ui-css` 内部实现
- [.vue 方言（指南）](/guide/scaling/sfc) · [声明式视图（参考）](/reference/reactive-view)
