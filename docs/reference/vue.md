# Vue 宿主接入

Vue + JavaScript 是 NanaUI 的一等 L1/L2 作者入口，与 Rust L3 声明式视图共享同一棵 `UiWorld`、同一份 `UiScene` 和同一张组件注册表。

你可以根据界面表面特点选择入口（Rust 入口见 [快速开始](../guide/quick-start.md)，概念对照见 [从 Vue 过来](../guide/from-vue.md)）：Vue 路径适合把按 Web 组件化习惯编写的页面直接落到**同一棵原生保留树**上。虽然开发体验接近 Web，但底层没有 WebView，因此不能把面向 `@vue/runtime-dom` 的普通网站构建产物原样塞进来当桌面应用运行。

## 应用如何接入

JavaScript 侧入口来自 `@nanaui/nanavue-runtime` 的 `createApp()`。应用使用自己的 Vite 工程将 SFC、TypeScript 与 CSS 打包为 Nana 可加载的脚本（通常为 IIFE 格式）；NanaUI 不扫描 `dist` 目录，也不捆绑第二套前端打包器。开发期免重启热重载配置见 [开发期热重载](hot-reload.md)。

```js
import { createApp } from "@nanaui/nanavue-runtime";
import { NanaButton } from "@nanaui/nanavue-components";
import "@nanaui/nanavue-components/controls.css";

createApp({
  // 根组件定义
}).mount();
```

Rust 宿主侧引入 `nana_ui_vue::prelude`，通过 `VueRuntimeProgram::run`（或 `mount_vue_as_nana`）将脚本产物与 V8 引擎交给同一个 `run_runtime`（`VueRuntimeProgram` 需启用 `hosted` feature，它隐含 `scene-view` 并通过 `SceneWgpuPainter` 绘制）。后续新开的窗口默认共享同一套 JavaScript 全局环境，也可按需请求独立的隔离上下文——无论哪种模式都共享同一个 V8 引擎实例与同一份 GPU 上下文。

## 两阶段启动（Early Splash）

Early Splash 在 Rust 侧通过 `nana_ui::with_startup(StartupOptions::default().with_splash(spec), || VueRuntimeProgram::run(...))` 配置。JS bundle 在 `UiReady` 阶段求值，此时原生启动 Logo 已经在屏幕上显示。

前端通过 `Nana.startup` 访问宿主启动状态的投影：

- `Nana.startup.state`：随时可读的最新状态快照 `{ phase, splash, ticket, timeline }`；
- `Nana.startup.deferTakeover()`：仅在脚本首次求值时有效，声明推迟接管画面；
- `Nana.startup.takeOver()` / `cancelTakeover()`：发起或撤回画面接管请求；
- `Nana.startup.onChange(listener)`：订阅宿主的 `startup` 阶段变更事件。

注意：Vue 宿主的默认主题为 `Light`，而 `SplashBackground::System` 跟随操作系统明暗设置；若应用不跟随系统明暗，请显式为 splash 指定 `SplashBackground::Color(..)` 以免交接时底色跳变。完整启动合同见 [两阶段启动](startup.md)。

## 章节导航

Vue 宿主的详细合同按主题拆分为以下章节：

| 主题 | 所在章节 | 核心内容 |
| --- | --- | --- |
| **多窗口与 JS 隔离** | [Vue 窗口](vue-windows.md) | `shared` 与 `isolated` 上下文、`tag`、桌面阴影 `shadow`、每窗口独立 `locale` 与窗口句柄权限 |
| **标签映射与 CSS 子集** | [Vue 宿主边界 · 两种写法](vue-host.md#两种写法同一棵树) | `Nana*` 控件与 HTML 语义标签如何落入 `ComponentRegistry`、`@container`、地标无障碍角色与单一输入路由 |
| **长列表性能与 Hover** | [Vue 宿主边界 · 长列表上的 hover](vue-host.md#长列表上的-hover) | 单渲染函数下 `v-for` 的重算开销、拆分子组件收益与常数级输入成本（详据见 [输入成本](input-cost.md)） |
| **本地化与 Markdown** | [Vue 宿主边界 · 本地化文字](vue-host.md#本地化文字) | `<T>` 组件、`Nana.i18n` 目录与回退链、子树 `locale` 作用域，以及 `NanaMarkdown`（`rich-text` feature） |
| **Web API 子集与网络存储** | [Vue 宿主边界 · Web 面与网络](vue-host.md#它提供的-web-面以及明确没有的) | 流式 `fetch`、`FormData`、`WebSocket`、`AudioContext` PCM 子集、源白名单、`Nana.storage` 与自定义宿主命令 |
| **扩展自定义控件** | [Vue 宿主边界 · 扩展控件](vue-host.md#扩展控件) | Rust `register_component` 与 JS `NativeComponentRegistry` 的职责边界 |
