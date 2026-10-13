# Vue 宿主边界

本文规定 Vue 路径在同一棵 `UiWorld` 上支持的标签映射、响应式容器查询、本地化文字、Web API 子集、网络/存储白名单以及自定义控件扩展方式。基础接入见 [Vue 宿主接入](vue.md)，多窗口管理见 [Vue 多窗口与上下文隔离](vue-windows.md)。

## 两种写法，同一棵树

在 Vue 模板中可以混合使用显式的 Nana 控件与标准 HTML 骨架标签，它们最终都通过 `ComponentRegistry` 和 Style Model 落入同一棵保留树：

1. **显式 Nana 控件**：`NanaButton`、`NanaInput`、`NanaDialog` 等组件直接映射到对应的 Runtime 控件。对话框、抽屉、上下文菜单等浮层请务必使用对应的 Nana 控件，**不要用 `position: fixed` 手搭网页式浮层**。
2. **HTML 语义标签与骨架标签**：
   - 与 Runtime 控件语义一致的 HTML 标签直接落为对应控件：`button`、`a`、`input`（含 `checkbox`、`radio`、`range`、`number`）、`textarea`、`select` + `option`、`ul` / `ol` / `menu` / `li`、`table` / `tr` / `td` / `th`、`progress`、`meter`、`hr`、`dialog`、`details` / `summary`。
   - 结构与文本骨架标签落为 `Column` 或 `Text`：如 `div`、`section`、`figure`、`figcaption`、`hgroup`、`address`、`picture`、`datalist`、`slot`、`map`。
   - 未识别的未知标签或已退役的旧别名会直接报错，不会静默退化为普通布局盒；自定义插件标签必须先在 Rust 侧调用 `register_component` 注册。
   - `v-html` 会将片段解析为保留树子节点；`Teleport to="body|html"`、`Transition`、`KeepAlive`、`Suspense` 均通过同一套 host ops 工作，不存在第二棵树。

### 媒体、替换元素与无障碍地标（Landmark）

- **媒体与替换元素**：`<video>` 与 `nana-video` 在带有 `data-nana-video` 时走宿主推帧通道（`nana.video`，槽位为 `video:{id}`，有槽位时不画 `poster`）。`<iframe>`、`<audio>`、`<embed>`、`<object>` 不伪造浏览器行为，仅作为不可见布局盒（PCM 音频播放请使用下文的 `AudioContext` 子集）；`<source>`、`<track>`、`<area>`、`<col>`、`<colgroup>` 显式跳过。应用内网页内容只能使用 Runtime `BrowserView` 的宿主原生例外（当前仅 macOS 后端，详见 [实时画面 · 应用内浏览器](gpu.md#应用内浏览器)）。
- **无障碍地标角色**：HTML 地标标签自动携带对应的无障碍角色——`nav` 为 `navigation`，`main` 为 `main`，`aside` 为 `complementary`，`search` 为 `search`，`header` 为 `banner`、`footer` 为 `contentinfo`（当 `header` / `footer` 位于 `article`、`aside`、`main`、`nav`、`section` 内部时除外）；`section` 与 `form` 仅在拥有可访问名称（来自 `aria-label`、`aria-labelledby` 或自身文本）时才成为 `region` 或 `form`。显式的 `role` 属性或语义类名（如 `<nav role="tablist">`）优先于标签推断。注意 `<search>` 与 `<form>` 在视觉上仍是普通布局盒，搜索下拉与表单行控件请用 `search-dropdown` 与 `form-field`。

### `@container` 容器查询与单一输入路由

- **`@container` 响应式规则**：元素命中的 `@container` 规则（支持子集见 [布局（CSS 子集）](layout.md#能用的)）会被编译为一条 Runtime 响应式规则；容器缩放时由 Runtime 直接切换尺寸桶，无需重跑 CSS 引擎。容器规则修改的字号、颜色等可继承文本属性由 Runtime 向下传递（但后代用 `em` 声明的字号仍按桶外字号换算；`direction`、`writing-mode`、`text-decoration`、`overflow-wrap` 不向下传递）。若单个元素的规则同时查询两个容器或两条轴，则整条规则不生效并记入不支持的 CSS 报告。
- **单一输入路由**：原生窗口输入、`VueHostedRuntime::runtime_input` 以及 `VueHost` 的 `dispatch_*` / `commit_text` 全部**先经过 Runtime 路由**，随后页面才收到 `pointer*`、`key*`、`wheel`、`input`、`composition*` 事件。因此焦点、Tab 遍历顺序（仅注册为控件的标签可聚焦，`tabindex` 不参与）、滚动和文本编辑均由 Runtime 裁决；页面在 `wheel` 或 `keydown` 中调用 `preventDefault` 无法拦截已经发生的滚动或输入。

## 长列表上的 Hover 与输入成本

在 Vue 路径中，单次指针事件的派发成本本身是常数（约 `0.062 ms`，Rust L3 约为 `0.0003 ms`），不随树规模增长。唯一容易在长列表上写出 $O(N)$ 耗时的是**让拥有整列 `v-for` 的渲染函数响应 hover 状态**：

- 模板中的 `v-for` 编译后属于同一个组件的 render function；若 `onPointerenter` 修改了参与该组件渲染的 `ref`，Vue 会重建整列的 vnode（实测 2,000 行时单次鼠标移动耗时约 `4.8–8.6 ms`）。
- 将每一行拆分为独立子组件并各自持有 hover `ref`，可省去外层列表的 vnode diff（2,000 行耗时降至约 `1.8 ms`）。
- 若希望长列表上的指针移动保持纯粹的 $O(1)$ 常数开销，请使用 CSS `:hover` / 组件内置交互态，或让 JS 监听器只修改不参与视图重渲染的状态。完整实测与优化记录见 [输入成本](input-cost.md)。

## Markdown 与本地化文字（`<T>` / `Nana.i18n`）

### Markdown（`NanaMarkdown`）

`NanaMarkdown` 通过 `modelValue` 或 `value` 接收原始 Markdown 文本（需启用 `rich-text` feature）。当源码未变化时保留解析结果与文本选区；源码变化后重新解析，代码高亮走共享的 Runtime 呈现绑定路径。

### 本地化文字（`<T>` 与 `Nana.i18n`）

在模板中使用 `@nanaui/nanavue-components` 导出的 `<T>`（别名 `NanaT`）声明本地化文本：

```vue
<script setup>
import { T } from "@nanaui/nanavue-components";
</script>

<template>
  <T id="files" :count="files.length" />
</template>
```

- **属性与参数传递**：`id` 为消息目录中的键名，其余属性直接作为消息参数（如 `:fileCount="n"` 对应模式串中的 `{fileCount}`）。`class`、`style`、`lang`、`dir`、`locale`、`data-*`、`aria-*` 及事件监听器保留在元素自身上；若参数名与这些保留属性重名，请放进 `:args="{ class: role }"`（`args` 优先于同名属性）。
- **Runtime 原地解析**：`<T>` 渲染为一个携带消息键与参数的 `<nana-text>` 节点，由 Runtime 按该节点所在作用域的 `locale` 直接解析。切换语言时由 Runtime 原地重算文本并仅重排受影响的盒子，**Vue 组件本身不需要重新渲染**。
- **参数类型与格式化**：
  - 数字（用于 `plural` 与 `{n, number}`）必须动态绑定 `:count="3"`（写 `count="3"` 会被当作字符串）；
  - 布尔值序列化为 `"true"` / `"false"` 供 `select` 分支匹配；
  - 日期支持 `Date` 实例（取本地年月日时分秒）或纯日期对象 `{ year, month, day }`（月份从 1 开始）；
  - 货币金额传入 `{ amount, currency }`（`currency` 为 ISO 4217 代码）；
  - `null` 与 `undefined` 视为缺失参数，输出中保留占位参数名。ICU 模式串语法详见 [文本引擎](text-engine.md)。

应用级消息目录与默认语言通过 `Nana.i18n` 配置（也可以在脚本执行前由 Rust 宿主的 `VueRuntime::set_message_catalog` / `set_default_locale` / `set_fallback_locale` 预设）：

```js
Nana.i18n.setCatalog(
  {
    "en-US": { files: "{count, plural, one {# file} other {# files}}" },
    "zh-CN": { files: "{count} 个文件" },
  },
  { fallback: "en-US", missing: "key" }, // missing 可选 "key"（默认）或 "keep-previous"
);
if (!Nana.i18n.locale) {
  Nana.i18n.setLocale("zh-CN");
}
```

`setLocale` 接收语言标签字符串，或细粒度配置对象 `{ messages, language, direction, formatting }`（分别控制消息表、OpenType 排版语言 `locl`、排版方向与数字/日期格式；未指定的项跟随 `messages`）。局部子树可通过 `<section locale="ar">` 覆盖语言与排版方向（而 HTML `lang` 属性仅影响字体回退与 `locl`，不切换消息目录）。注意 `<T>` 生成的是 `<nana-text>` 子节点，因此可用于按钮等渲染子节点的容器，但不能用于只读取纯字符串属性的槽位（如标题栏标题字符串或按钮自身的 `accessible_name` 属性）。

## 它提供的 Web API 子集，以及明确不提供的能力

这些 Web 兼容接口旨在让熟悉的组件代码顺畅跑在桌面窗口里，而不是为了复刻完整的浏览器内核：

| 类别 | 已提供的子集能力 | 明确不提供的浏览器/Tauri 能力 |
| --- | --- | --- |
| **DOM / 全局** | `window` / `document` 受限子集、事件、定时器、`requestAnimationFrame`、桌面剪贴板 | 完整 DOM / CSSOM、Cookie、浏览器 CORS 预检、Service Worker |
| **网络** | 受白名单管控的流式 `fetch`（含 `ReadableStream`、`FormData`）、`WebSocket` | 流式**请求**体（`duplex`）、BYOB reader、无白名单的任意外网访问 |
| **存储** | 应用命名空间内的 `localStorage` 与 JSON 助手 `Nana.storage`（注入 `FileStore` 后落盘） | `IndexedDB`（调用抛 `NotSupportedError`）、跨命名空间枚举或篡改框架状态 |
| **音频 / 媒体** | Web Audio PCM 子集（`AudioContext`、`AudioBuffer`、`AudioBufferSourceNode`、`GainNode`、`ScriptProcessorNode`；桌面端需启用 `native-audio` feature 走 cpal） | 完整 Web Audio 节点图、空间化音频、`AudioWorklet`、`decodeAudioData`、`<audio>` 解码器、Tauri `invoke`/插件协议 |

### 流式 `fetch`、`FormData` 与 `WebSocket`

`fetch(url)` 在收到**响应头**时即 resolve，响应体随后分块在每帧 `pump_frame` 中送达引擎线程，可通过 `response.body.getReader()` 流式读取（`text()`、`json()`、`arrayBuffer()`、`blob()` 与 `clone()` 仍保持标准完整读取语义；一旦调用了 `getReader()`，再调 `text()` 或 `clone()` 会按标准抛出 `TypeError`；`101 / 204 / 205 / 304` 响应的 `body` 为 `null`）：

```js
const response = await fetch(url);
const reader = response.body.getReader();
for (;;) {
  const { value, done } = await reader.read();
  if (done) break;
  // value 为 Uint8Array 分块
}
```

两点边界说明：
1. **无反向背压**：为支持 `clone()` 各自独立消费，`BodySource` 会保留已到达的分块，`desiredSize` 仅反映尚未读走的分块数；
2. **共享累计字节上限**：流式读取旨在尽早开始处理数据，不能绕过宿主的 `FetchPolicy` 响应体积上限（内置 `NativeFetchHost` 默认 `16 MiB`），累计超出上限会在流中途以 `ResponseTooLarge` 终止。

`FormData` 可直接作为 `fetch` 请求体（自动编码为 `multipart/form-data` 并生成 boundary，文件项传入 `Blob`；不支持 `new FormData(formElement)`）。`WebSocket` 由桌面端内置的 `NativeWebSocketHost` 驱动，默认 `SocketPolicy` 同样为空白名单，需通过 `MountOptions.socket_host` 或 `WebApiState::set_socket_host` 注入允许的源。

## 网络安全策略、持久化存储与宿主命令

- **网络默认全关（Fail-closed）**：必须显式按 `scheme://host[:port]` 格式登记允许的 origin（`localhost` 不会自动放行）。跨源重定向会自动剥离授权类请求头；默认超时 `30 s`，请求与响应上限各 `16 MiB`，最多 `5` 次重定向。同一个 `fetch_host` 同时管控 JS `fetch()` 与样式/页面中的 `url()`、`<img src>`、`mask-image`、`border-image`（未注入 host 时拒绝一切远程图片，仅放行 `data:`、`file:` 与 jail 内相对路径；`@font-face` 永远不走远程网络）。
- **物理存储注入**：默认存储位于内存中；通过 `FileStore::open(app_data_dir("YourApp")?)` 注入持久化后端后，主上下文的 `localStorage` 与 `Nana.storage` 落盘为 `local-storage.bin`，且严格限制在应用自己的命名空间内（Dock 与窗口几何归 `ViewStateStore`，外观设置归 `Settings` 命名空间）：

```rust
VueRuntimeProgram::run_with_store(
    settings.persist_key("main"),
    engine,
    artifact,
    application_api,
    shared_store(FileStore::open(app_data_dir("YourApp")?)?),
)?;
```

- **自定义宿主命令**：应用可通过 `HostApiRegistry` 注册自定义命令供前端调用（不可与框架内置宿主接口重名，否则启动直接报错）。

## 扩展自定义控件

若要让新增的自定义控件完整参与布局、命中与绘制，请在 Rust 侧实现并调用 `register_component`：其在 Vue 模板中的标签名即为 `ComponentTypeId` 去掉 `nana.` 前缀（例如 `nana.preview-card` 对应 `<preview-card>`）。如果只需要向 JS 暴露一组命令而不需要新的保留树控件类型，才使用 `NativeComponentRegistry`——两张注册表分属不同边界，只登记其中一张不会在另一条路径上生效（详见 [控件合同](components.md)）。
