# Vue 多窗口与上下文隔离

本文说明在 Vue 宿主中如何创建多扇窗口、选择 JavaScript 上下文隔离级别，以及配置窗口标签（`tag`）、桌面阴影（`shadow`）与窗口级语言（`locale`）。基础接入方式见 [Vue 宿主接入](vue.md)。

## 共享上下文 vs 隔离上下文（`isolation`）

调用 `Nana.windows.create({ isolation })` 创建新窗口时，可通过 `isolation` 选择 JavaScript 上下文模式：取值为 `"shared"`（默认）或 `"isolated"`（拼写错误的取值会直接报错，不会静默回退到共享模式）。

### 1. 共享模式（`"shared"`，默认）

新窗口与打开它的脚本运行在**同一个 JS 全局环境**中：

- **跨窗口共享的状态**：JS 全局变量与模块状态（Vue / Pinia store、已注册组件）、`Nana.host.on` 监听器与宿主事件队列、`setTimeout` / `setInterval` / `requestAnimationFrame` 的回调表与 ID、`fetch` / `WebSocket` 的 JS 对象、`localStorage`、宿主资源句柄、`Nana.dialogs` provider 以及 `Nana.components.onError` 监听。
- **每扇窗口独立持有的状态**：底层 `RuntimeDocument`、`window` 与 `document` 外观对象、`sessionStorage`、定时器与网络请求在宿主侧的生命周期调度、焦点链以及 IME 会话。

### 2. 隔离模式（`"isolated"`）

新窗口获得独立的 JavaScript Realm，**同一份应用脚本会在新上下文中重新完整执行一遍**。全局变量、模块状态、定时器与内存中的 `localStorage` / `Nana.storage` 与其他窗口完全隔离（窗口关闭即释放）。在隔离窗口内部再打开的 `"shared"` 窗口归属于该隔离上下文；`Nana.windows.list()` 仅列出当前上下文可见的窗口。当一个隔离上下文中的最后一扇窗口关闭且完成 `window-closed` 卸载后，该上下文即被销毁。

脚本通过 `Nana.windows.current()` 识别自身所在的窗口与初始参数：

```js
const current = Nana.windows.current(); // { id, isolation, tag, params, ... }
createApp(current.params?.view === "settings" ? Settings : Main).mount();
```

创建隔离窗口时可通过 `params` 传入可 JSON 序列化的初始数据（包含循环引用会在 `create()` 时抛出 `TypeError`；`params` 仅对隔离窗口生效）：

```js
const settings = await Nana.windows.create({
  isolation: "isolated",
  params: { view: "settings" },
});
settings.focus();
```

打开方拿到的窗口句柄可用于窗口级控制（`focus`、`close`、`setBounds`、`setLocale`、`ready`、`closed`），但**不能跨上下文调用 `mount`**（隔离窗口的内容由其自身上下文挂载，句柄上的 `window`、`document`、`root` 均为 `null`）。

### 两种模式的共同底层与边界

无论选择 `"shared"` 还是 `"isolated"`，以下底层设施始终跨窗口共享：

- GPU `Device` 与 `Queue`，以及 WebGPU、Canvas、SVG、媒体与视频运行时；
- 原生组件注册表与 `HostTextureRegistry`、应用样式表、消息目录与应用级 locale（`Nana.i18n`）、动画时钟、诊断输出，以及宿主注册的 Rust 命令（`HostApiRegistry`）。

::: warning 隔离的是状态，不是线程或安全沙箱
所有上下文共享同一个 V8 堆、同一个执行线程与同一个微任务队列——某扇窗口里的死循环仍会阻塞全部窗口，窗口控制也不是进程安全边界。此外，隔离窗口要求应用脚本以源码形式加载；使用 V8 snapshot 或不支持多 Realm 的引擎时，`create({ isolation: "isolated" })` 会以 `WindowOpenError` 失败。
:::

## 窗口标签（`tag`）、桌面阴影（`shadow`）与窗口级 `locale`

### 窗口种类标签（`tag`）

共享窗口与隔离窗口均可通过 `tag` 声明窗口种类（字符串）。它原样映射为 `WindowDescriptor::tag`，出现在 `create()`、`current()`、`list()` 返回的句柄上（未设置时 `handle.tag` 为 `null`），Rust 侧也可通过 `RuntimeProgramContext::window_tag()` 读取（主窗口的 `tag` 来自传给 `VueRuntimeProgram::run` 的描述符，脚本首次执行时即可从 `Nana.windows.current().tag` 读到）：

```js
const tracking = await Nana.windows.create({ tag: "tracking", role: "tool" });
tracking.mount(tracking.tag === "tracking" ? TrackingPanel : Main);
```

### 桌面阴影（`shadow`）

`shadow` 控制原生窗口阴影：

- `"auto"`（默认）：普通窗口使用系统阴影，透明窗口自动使用跟随根卡片几何的阴影；
- `"none"`：关闭窗口阴影；
- 自定义阴影对象：`{ color: [r, g, b, a], offset: [x, y], blur, spread, source: "windowShape" }`（缺省字段取默认值；拼写错误的关键字、未知字段或非法数值会直接报错）。实际生效结果以宿主返回的呈现状态为准（详见 [窗口合同](window.md)）。

### 每扇窗口独立的 `locale`

创建窗口时传入 `locale`（或之后调用句柄的 `handle.setLocale(locale)`）可为该窗口单独指定语言环境，覆盖应用级的 `Nana.i18n.setLocale`；传入 `null` 则恢复跟随应用级 locale。切换时只有这扇窗口内的 `<T>` 本地化文本会重新解析，其他窗口不受影响；若指定了从右到左（RTL）的 locale，该窗口根节点会自动按从右到左排版（详见 [Vue 宿主边界 · 本地化文字](vue-host.md#本地化文字)）：

```js
const help = await Nana.windows.create({ title: "帮助", locale: "ar" });
help.setLocale(null); // 恢复跟随应用级 locale
```

## 全屏与置顶状态的异步回流

`windowSetFullscreen` 与 `windowSetAlwaysOnTop` 仍接收布尔值，但 `windowGeometry().fullscreen` 与窗口的 `alwaysOnTop` 属性反映的是**宿主实际观察到的生效状态**（通过 `WindowEvent::ModeChanged` 回流），在请求入队瞬间不再做乐观改写。因此在调用设置方法后立即同步读取几何状态，拿到的可能仍是变更前的旧值。

窗口化多窗口对照示例见 `examples/vue-hosted-acceptance`（`examples/vue-counter` 仅作为含无头点击的引擎探针，不作为应用模板）。
