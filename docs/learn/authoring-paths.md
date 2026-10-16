# 三种作者入口

NanaUI 提供三种编写界面的方式，但底层始终只有**一棵保留树（`UiWorld`）**和**一个绘制器（`SceneWgpuPainter`）**。本章拆解三条入口各自的链路、在哪个 crate 分岔、在哪里汇合，并给出「为什么不会长出第二棵树」的源码级证据。

## 全局汇合图

```text
入口一（Rust）     view! / css! / stylesheet! 与声明式视图函数
                      │ nana-ui-view-macros → view-codegen（构建时调用 CSS 引擎）
                      ▼
入口二（Vue）      Vue 组件（nanavue-runtime JS 包，运行在嵌入的 V8 中）
                      │ Custom Renderer host ops（经 nana-js-engine 挂接）
                      ▼
入口三（JS 宿主）  直接调用 JS Host API（nana-ui-web-api）
                      │
                      ▼
        PendingHostOps（MutationQueue：nana-ui-runtime 的 UiMutation 词表）
                      │ flush_host_frame / 帧边界原子 commit
                      ▼
        UiWorld（nana-ui-runtime，唯一权威保留树）
                      ▼
        UiScene → SceneWgpuPainter（nana-ui，唯一绘制器）
```

三条入口是**三种输入合同**，而非三个独立运行时：它们的样式统一汇入同一份 Style Model（Tokens + Semantics + Layout），节点全部写入同一棵 `UiWorld`。

## 入口一：Rust（`view!` 宏链与函数式视图）

在 Rust 路径中，既可以使用 `<Tag>` 模板语法，也可以使用等价的函数式构建器（如 `column().gap(8).children(...)`）：

```rust
use nana_ui::runtime::view;

view! {
    <Column gap=8>
        <Button @activate={save}>"确定"</Button>
    </Column>
}
```

构建时编译链路如下：

| 阶段 | Crate | 职责说明 |
| --- | --- | --- |
| 1. 过程宏入口 | `nana-ui-view-macros` | 解析 `view!`、`css!`、`stylesheet!` 语法树 |
| 2. 控件描述表 | `nana-ui-view-schema` | 提供内置控件标签、字段与事件的唯一中间表示（`for_each_control!`） |
| 3. 代码生成 | `nana-ui-view-codegen` | 展开为 Runtime 视图函数调用；`<style>` 与 `css!` 中的 CSS **在构建时**调用 `nana-ui-css` 解析，直接生成 Style Model 补丁数据 |

源码证据：

- `crates/nana-ui-runtime/src/view/mod.rs:151-162`：`stylesheet!` 宏在 Runtime 中定义并转发给 `nana_ui_view_macros`：
  ```rust
  macro_rules! stylesheet { ... }
  pub use nana_ui_view_macros::stylesheet as __stylesheet;
  ```
- 运行时拿到的是**纯数据编译产物**：`crates/nana-ui-runtime/src/view/style.rs:3-8` 模块文档明确指出 `.vue` 编译器与 `css!` 在构建时解析并匹配视图样式表，运行时不解析 CSS、不匹配选择器，本 crate 不含 CSS 引擎。
- 依赖图佐证：`nana-ui-runtime` 与 `nana-ui` 的普通依赖表中均没有 `nana-ui-css`，仅有 `view-macros` 与 `view-schema`（详见 [模块与分层](/learn/modules-and-layers)）。

## 入口二：Vue（Custom Renderer）

Vue 组件使用 JavaScript / TypeScript 编写（仓库内 `packages/` 提供 `@nanaui/nanavue-runtime` 与 `@nanaui/nanavue-components`），运行在嵌入的 V8 引擎中。Vue 3 的 Custom Renderer 将 `createElement`、`insert`、`patchProp` 等操作转化为 **host ops**，经 `nana-js-engine` 的 `HostApiRegistry` 挂接到 Rust 侧：

- `crates/nana-ui-vue/src/renderer.rs:43`：`register_dom_host_ops(api: &mut HostApiRegistry, doc: Arc<Mutex<NanaTreeDocument>>)` 将 DOM host ops 注册进引擎并写入 `NanaTreeDocument`。
- 该模块头部文档（`renderer.rs:1-8`）写明自我约束："Identity/hierarchy go to `UiWorld`. This module does not build a second retained tree."

写入路径分为两层：

1. **门面层（`NanaTreeDocument`）**：提供 DOM / CSS 兼容查询外观。每次写入推进 `PendingHostOps`（`crates/nana-ui-vue/src/tree.rs:220-244`）——底层是一个 `MutationQueue` 外加几张只读穿写镜像表（`parent` / `children` / `kinds` / `texts`，供 JS 门面同步查询）。
2. **提交层（Commit）**：`flush_host_frame`（`tree.rs:753`）在帧边界将队列一次性 commit 进 `RuntimeDocument`，随后排空 Runtime 系统工作。`PendingHostOps` 的注释明确规定镜像表与队列必须**成对清空**，严格保持锁步（lockstep）。

`UiMutation` 词表本身定义在 **`nana-ui-runtime`**（`crates/nana-ui-runtime/src/mutation.rs:15`，`MutationQueue` 位于 `:306`）：`Create`、`Insert`、`Detach`、`SetStyle`、`StartAnimation`、`RequestFocus`……Vue 所能执行的每一项操作都是 Runtime 已经定义好的标准变异，**Vue 没有属于自己的第二套树操作原语**。

两个关键结构性证据：

- `VueRuntime` 对 `UiWorld` 直接实现 `Deref` / `DerefMut`（`tree.rs:206-218`）——Vue 侧持有的「运行时」直接解引用到 Runtime 的 `UiWorld`，而非另一个独立世界。
- `MessageBridge` 中的层级仅是**级联投影索引**（`crates/nana-ui-vue/src/bridge.rs:9-19`）："Observable parent/children/roots are overwritten from `UiWorld` by `apply_runtime_hierarchy` before Scene paint"，"UiWorld remains authoritative for drawing, hit testing, focus, accessibility, and the final observable hierarchy"。

在这条路径上，CSS 在**运行时**解析：`nana-ui-vue` 依赖 `nana-ui-css`，由 `css_cascade` 在运行时匹配样式表（这是唯一 CSS 引擎的两个消费时机之一，另一个是构建时的 `view-codegen`）。

## 入口三：JS 宿主（不经过 Vue）

JS 代码也可以直接调用宿主 API（`nana-ui-web-api`：DOM 子集、`fetch`、定时器、Web Audio PCM 等）。它与 Vue 路径共享同一个 `NanaTreeDocument`、`MessageBridge` 与 `LayoutBoxStore`（见 `renderer.rs:31-40` 的 `HostDocs` 结构）。引擎抽象合同位于 `nana-js-engine`（`JsEngine` trait，同时作为单元测试注入缝），生产实现位于 `nana-js-v8`：

- `crates/nana-ui-vue/src/lib.rs:76-77`："Custom Renderer host ops attach through `nana_js_engine::JsEngine` only — never via `v8::*`"——所有宿主操作通道均收敛在引擎合同层。

## 输入同样是单向适配，而非旁路

Vue 页面收到的浏览器形状事件（`pointer*`、`key*` 等）是对**已经由 Runtime 路由完毕**的规范输入事件的观察：

- `crates/nana-ui-vue/src/hosted_adapter.rs:446-455`（`observe_routed_input`）：原生 `scene_host` 已经先把事件路由进 `UiWorld`，Vue 侧只是将路由结果翻译为页面事件回调。
- `hosted_adapter.rs:460-469`（`route_standalone`）：无原生窗口宿主的会话调用 `host.route_input(payload)`，底层依然进入同一个 `AppContext::route_input`。

## 「只有一棵树」核查读数

| 潜在破坏条件 | 当前工作区实测结果 |
| --- | --- |
| Vue / JS 维护独立的保留树结构 | **无。** `PendingHostOps` 只是 Runtime `UiMutation` 的帧内队列；`MessageBridge` 层级只是级联投影索引，绘制前由 `UiWorld` 覆写；`VueRuntime` 直接 `Deref` 到 `UiWorld`。 |
| Vue / JS 维护独立的绘制管线 | **无。** 绘制统一走 `scene-view` feature 下的 `nana-ui` Scene 宿主（`UiWorld → UiScene → SceneWgpuPainter`）。 |
| 存在第二套组件实例化 ABI | **无。** 内置与扩展组件统一走 `ComponentRegistry`；`NativeComponentRegistry` 仅服务于 JS 宿主命令，不是 Runtime 实例化 ABI。 |

## 给学习者的三个观察点

1. **找汇合点比找分岔点更重要**：三条入口在 `UiMutation` / `UiWorld` 完全汇合，因此只要掌握一遍保留树 API，三条路径的内部行为就全部贯通。
2. **门面（Facade）不等于权威**：Vue 的 DOM 外观（`NanaTreeDocument`）可供脚本读取，但盒子几何、指针命中与焦点链的最终答案永远从 `UiWorld` 投影而来；`LayoutBoxStore` 只是绘制后的诊断/查询快照，滚动偏移不写回 Runtime 布局盒。
3. **构建时 vs 运行时的 CSS**：同一个引擎（`nana-ui-css`），两个调用时机（`view-codegen` 构建时 / `nana-ui-vue` 运行时）。判断一段样式在哪里解析，看它从哪条入口进入，而不看样式内容本身。

## 接着读

- [保留树：UiWorld](/learn/runtime-tree)：三条入口汇合后的节点表与提交机制
- [CSS 引擎](/learn/css-engine)：构建时与运行时两个消费时机的内部流水线
- [端到端数据流](/learn/flows)：一次状态更新从模板/JS 到屏幕像素的完整旅程
- [Vue 宿主接入（参考）](/reference/vue)：面向应用开发者的 Vue 接入指南
