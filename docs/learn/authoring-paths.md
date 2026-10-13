# 三种作者入口

NanaUI 有三种写界面的方式,但只有**一棵树**和**一个 painter**。这一章讲三条入口各自的链路、在哪个 crate 分岔、在哪里汇合,并给出「为什么不会长出第二棵树」的代码证据。

## 总览

```text
入口一(Rust)      view! / css! / stylesheet!
                      │ nana-ui-view-macros → view-codegen(构建时,调用 css 引擎)
                      ▼
入口二(Vue)       Vue 组件(nanavue-runtime JS 包,跑在 V8 里)
                      │ Custom Renderer host ops(经 nana-js-engine 挂接)
                      ▼
入口三(JS 宿主)   直接的 JS host API(nana-ui-web-api)
                      │
                      ▼
        PendingHostOps(MutationQueue:nana-ui-runtime 的 UiMutation 词表)
                      │ flush_host_frame / 帧边界 commit
                      ▼
        UiWorld(nana-ui-runtime,唯一权威树)
                      ▼
        UiScene → SceneWgpuPainter(nana-ui,唯一 painter)
```

三条入口是**输入合同**,不是三个运行时。它们的样式都汇到同一份 Style Model(Tokens + Semantics + Layout),节点都进同一棵 `UiWorld`。

## 入口一:Rust(`view!` 宏链)

写法:

```rust
view! {
    Column {
        Button::new(cx, "确定")?.on_activate(|_, _, cx| { /* ... */ })
    }
}
```

链路(构建时):

| 阶段 | Crate | 做什么 |
| --- | --- | --- |
| 1. 过程宏 | nana-ui-view-macros | 展开 `view!`/`css!`/`stylesheet!` 语法树 |
| 2. schema | nana-ui-view-schema | 定义视图结构的中间表示 |
| 3. 代码生成 | nana-ui-view-codegen | 生成 Rust 代码;`<style>`/`css!` 里的 CSS **在构建时**调用 nana-ui-css 解析,输出 Style Model 数据 |

关键证据:

- `crates/nana-ui-runtime/src/view/mod.rs:151-162`——`stylesheet!` 宏在 runtime 里定义并转发给 `nana_ui_view_macros`:
  ```rust
  macro_rules! stylesheet { ... }
  pub use nana_ui_view_macros::stylesheet as __stylesheet;
  ```
- 运行时拿到的是**编译产物**:`crates/nana-ui-runtime/src/view/style.rs:3-8` 模块文档写明 *".vue 编译器(和 `css!`)在构建时解析并匹配视图的样式表……运行时不解析 CSS、不匹配选择器,本 crate 没有 CSS 引擎"*。
- 依赖表佐证:runtime 与 nana-ui 的依赖里没有 `nana-ui-css`,只有 view-macros/view-schema(见[模块与分层](/learn/modules-and-layers))。

## 入口二:Vue(Custom Renderer)

Vue 组件用 JS 写(仓库里的 `packages/` 提供 nanavue-runtime / nanavue-components),跑在嵌入的 V8 里。Vue 3 的 Custom Renderer 把 `createElement`/`insert`/`patchProp` 等操作变成 **host ops**,经 `nana-js-engine` 的 `HostApiRegistry` 挂到 Rust 侧:

- `crates/nana-ui-vue/src/renderer.rs:43`——`register_dom_host_ops(api: &mut HostApiRegistry, doc: Arc<Mutex<NanaTreeDocument>>)`:host ops 注册进引擎,写入 `NanaTreeDocument`。
- 该模块文档(`renderer.rs:1-8`)自我约束:*"Identity/hierarchy go to `UiWorld`. This module does not build a second retained tree."*

写入路径分两层:

1. **facade 层(`NanaTreeDocument`)**:DOM/CSS 外观。每次写推进 `PendingHostOps`(`crates/nana-ui-vue/src/tree.rs:220-244`)——一个 `MutationQueue` 加几张**只读镜像表**(parent/children/kinds/texts,供 facade 查询)。
2. **commit 层**:`flush_host_frame`(`tree.rs:753`)把队列 commit 进 `RuntimeDocument`,再 drain Runtime 系统。`PendingHostOps` 的注释明确:镜像与队列必须**成对清空**,保持 lockstep。

`UiMutation` 这个词表本身定义在 **runtime**(`crates/nana-ui-runtime/src/mutation.rs:15`,`MutationQueue` 在 `:306`):Create/Insert/Detach/SetStyle/StartAnimation/RequestFocus…… Vue 能做的每件事都是 runtime 已经定义的变异。**Vue 没有自己的树操作原语。**

两个结构性证据:

- `VueRuntime` 对 `UiWorld` 是 Deref/DerefMut(`tree.rs:206-218`)——Vue 侧的「运行时」就是 runtime 的世界句柄,不是另一个对象。
- `MessageBridge` 的层级只是**级联投影索引**(`crates/nana-ui-vue/src/bridge.rs:9-19`):*"Observable parent/children/roots are overwritten from `UiWorld` by `apply_runtime_hierarchy` before Scene paint"*;*"UiWorld remains authoritative for drawing, hit testing, focus, accessibility, and the final observable hierarchy"*。

CSS 在这条路径是**运行时**解析的:nana-ui-vue 依赖 nana-ui-css,`css_cascade` 在运行时匹配样式表(这是 CSS 引擎的两个消费时机之一,另一个在构建时的 view-codegen)。

## 入口三:JS 宿主(不经 Vue)

JS 代码也可以直接调宿主 API(`nana-ui-web-api`:DOM 兼容、fetch、计时器等)。它与 Vue 路径共享同一个 `NanaTreeDocument`/`MessageBridge`/`LayoutBoxStore`(见 `renderer.rs:31-40` 的 `HostDocs` 结构)。引擎合同在 nana-js-engine(`JsEngine` trait,同时是测试注入缝),产品实现是 nana-js-v8。

- `crates/nana-ui-vue/src/lib.rs:76-77`:*"Custom Renderer host ops attach through `nana_js_engine::JsEngine` only — never via `v8::*`"*——连 host ops 的通道都收敛在合同层。

## 输入也是适配,不是旁路

Vue 页面看到的浏览器形状事件(pointer/keyboard/…)是对**已经路由完**的规范输入的观察:

- `crates/nana-ui-vue/src/hosted_adapter.rs:446-455`——`observe_routed_input`:原生 scene host 已经把事件路由进 UiWorld,Vue 侧只是把结果翻译成页面事件。
- `hosted_adapter.rs:460-469`——`route_standalone`:无原生宿主的窗口走 `host.route_input(payload)`,仍是那一个 runtime 路由。

## 「一棵树」核查读数

| 推翻条件 | 实测 |
| --- | --- |
| vue/js 有独立保留树结构 | 无。`PendingHostOps` 只是 runtime `UiMutation` 的队列;`MessageBridge` 层级是投影索引,paint 前被 UiWorld 覆盖;`VueRuntime` 直接 Deref 到 `UiWorld` |
| vue/js 有独立绘制管线 | 无。绘制走 `scene-view` feature 下的 nana-ui Scene host(Runtime/UiScene → WGPU);`renderer.rs:8`:*"绘制经 Runtime / UiScene → nana-ui Scene host"* |
| 有第二套实例化 ABI | 无。`NativeComponentRegistry` 只服务 JS host 命令;`PendingHostOps` 镜像、`WidgetKind`、`ComponentSupport` 都不是 ABI |

## 给学习者的观察点

1. **找汇合点比找分岔点重要**:三条入口在 `UiMutation`/`UiWorld` 汇合,所以学一遍树 API,三条路径全通。
2. **facade ≠ 权威**:Vue 的 DOM 外观(`NanaTreeDocument`)可以查询,但几何、命中、焦点的答案永远从 UiWorld 投影回来。`LayoutBoxStore` 是绘制后的诊断快照,滚动不写回 Runtime 布局盒。
3. **构建时 vs 运行时的 CSS**:同一个引擎(nana-ui-css),两个时机(view-codegen 构建时 / vue 运行时)。判断一段样式在哪解析,看它的入口,不看内容。

## 接着读

- [保留树:UiWorld](/learn/runtime-tree):汇合点长什么样
- [CSS 引擎](/learn/css-engine):两个时机的细节
- [端到端数据流](/learn/flows):一条 Vue 更新从 JS 到像素的完整路径
- [Vue(参考)](/reference/vue):面向应用的 Vue 合同
