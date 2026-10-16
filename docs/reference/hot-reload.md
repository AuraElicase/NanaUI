# 开发期热重载合同

本文说明开发期热重载（`nana-ui-dev`）在 Vue L1/L2 与 Rust L3 两条路径上的响应延迟、内部执行顺序、状态交接机制以及为什么 Rust L3 采用进程重执（re-exec）而非 dylib 热替换。快速配置步骤见指南中的 [热重载](../guide/scaling/hot-reload.md)。

## 各层改动方式与实测延迟

`nana-ui-dev` 顶部带有编译期门禁 `#[cfg(not(debug_assertions))] compile_error!(...)`——而发布配置 `[profile.dist]` 继承自 `release`，因此一旦产品正式构建误引入该 crate 会直接编译失败，绝无可能将文件监视器带入用户交付物。

| 修改的内容 | 触发的重载动作 | 实测量级 |
| --- | --- | --- |
| **Vue：已注册的独立 `.css`** | **原位替换样式表（零拆树）**：节点 ID、焦点、滚动偏移、GPU 槽位与进行中的动画全部保留 | `< 50 ms` |
| **Vue：`.vue` / `.ts`（打包产物变化）** | **重建 V8 Isolate 并重挂 UI 树**：原生窗口、位置、`Device` / `Queue` / `Surface` 与宿主纹理全部保留 | `~200 ms` |
| **Rust `.vue` 方言：仅改静态文字** | **替换静态文本表（`TemplateText::apply`）**：不重编译、不重启，节点与焦点原地保留 | `< 50 ms` |
| **Rust L3：修改 `.rs` 源码** | **后台增量编译 → 交接窗口几何与会话状态 → `re-exec`**：窗口仅闪动一次 | `1.5–3.6 s`（链接占大头） |

在 macOS（Apple Silicon，已预热的默认 `dev` 配置）上实测 `cargo build -p component-gallery`：仅修改应用 crate（1.8 万行）并重链接 131 MB 二进制耗时 **`1.5–1.6 s`**；修改底层 `nana-ui-runtime`（12.7 万行，连带重编 `scene` 与 `nana-ui`）耗时 **`3.6 s`**。Windows 平台链接耗时更长，建议在本地通过 `cargo build -p <包名> --timings` 实测并为 dev 配置开启 `lld`。

## Vue 路径：CSS 快路径与全量重载的 7 步顺序

在 `Cargo.toml` 中新增独立的 `dev` 二进制目标（启用 `dep:nana-ui-dev` 与 `nana-ui-dev/vue`），在 `DevConfig::new("web/dist/app.iife.js").watch("web/dist").css("web/dist/app.css")` 中显式注册独立产出的 CSS 文件（注意：Vite 的 IIFE 构建默认将 CSS 内联进 JS bundle，若不单独产出 CSS 文件则修改样式会触发全量 JS 重载），并在 `initialize` 中持有 `nana_ui_dev::watch_and_reload(&dev, context)?` 返回的监听器（旁边运行 `vite build --watch`）。

当 JS bundle 变化触发全量重载时，`crates/nana-ui-vue/src/dev.rs` 严格按以下 7 步顺序执行：

1. 调用 `globalThis.__nanaDevSaveState` 提取应用可选提供的保存状态串；
2. 关闭除主窗口外的所有辅助窗口（包括隔离窗口——所有窗口共享同一个即将被替换的引擎实例，辅窗由新脚本求值后自行恢复打开）；
3. 执行 `clearMount` 卸载保留子树（保留根部 `html` / `body` 脚手架），同时由 `UiWorld` 在同一次 despawn 中清理焦点、IME、指针捕获与动画；
4. 清理跨树存活的宿主状态：作者样式表、排队的 bridge 事件、定时器、`rAF`、在途 `fetch`、已打开的 `WebSocket` 与滚动偏移；
5. **先 `shutdown` 并析构旧 V8 引擎，再创建新 V8 引擎**（V8 Isolate 进入/退出要求严格的 LIFO 栈序，若先建新实例再丢旧实例会直接触发进程 abort）；
6. 重新初始化：注册全套 Host API、求值新 bundle（若新脚本存在语法错误导致求值失败，自动回退装载**上一份可用 artifact** 并通过诊断通道报错，杜绝无提示白屏）、重绑事件桥；
7. 恢复窗口几何与当前主题。

### 两个关键架构决策

- **全程不重新调用 `bind_host_gpu`**：GPU 上下文绑定在 `VueHost` 而非 JS 引擎上，因此 `Device`、`Queue`、`Surface` 与所有已注册的 `HostTexture` 在重载期间纹丝不动——这正是它比传统 WebView 整页刷新更快、更稳的核心原因。
- **为什么在原 `RuntimeDocument`（`UiWorld`）上原地重建，而不是换一个新 `RuntimeDocument`**：每扇窗口的 `HostedAccessibility` 只在建窗时创建一次，其内部的 `AccessibilityProjector` 靠单调递增的 `generation` 守卫更新；若换成一个 `generation` 归零、`StableNodeId` 从 1 重新分配的新 `UiWorld`，无障碍投影器会永久拒绝后续的每一次更新，静默打断系统读屏。

## Rust L3 路径：为什么采用进程重执（Re-exec）而非 dylib 热替换

Rust L3 不做进程内 `dlopen / dlclose` 代码热替换，因为在不破坏框架核心合同的前提下有五道硬障碍：

1. **`TypeId` 跨编译不稳定**：声明式视图与 keyed 装配依赖 `TypeId::of::<C>()`，重新编译出的 dylib 对同一类型会生成不同的 `TypeId`，导致所有 keyed 子节点失配重建、`remove_view` 直接失败；
2. **`RuntimeProgram` 不是对象安全的（not object-safe）**：`with_document` 为泛型闭包方法，无法放进 vtable，若跨 FFI 包装则等于引入 `AGENTS.md` 严令禁止的第二套 C-ABI；
3. **事件闭包持有指向旧代码段的裸函数指针**：安全卸载 dylib 前必须先拆毁整棵树，与重建无异；
4. **平台动态库卸载限制**：macOS 的 `dlclose` 不会真正卸载含有 Objective-C 元数据或线程本地存储（TLS）的镜像；
5. **全局静态量分裂**：dylib 若重复链接 `wgpu` 或 `v8` 会产生两套实例。

因此 Rust L3 的诚实方案是**后台增量编译 + 状态交接 + 原地 `re-exec`**（完整示例见 [`crates/nana-ui-dev/examples/l3-dev-entry.rs`](../../crates/nana-ui-dev/examples/l3-dev-entry.rs)）：

```rust
// 1. main 入口使用 run_with_restart 包装
nana_ui_dev::run_with_restart::<MyProgram>(settings, Path::new("target/nana-dev-handoff"))?;

// 2. initialize 中恢复上一轮交接的会话状态（首次启动或结构体字段变更反序列化失败时优雅回退 Default）
let session = nana_ui_dev::restored_handoff()
    .and_then(|handoff| handoff.state_as::<Session>())
    .unwrap_or_default();
let watcher = nana_ui_dev::watch_and_rebuild(
    &DevConfig::new_rust("src"),
    RebuildCommand::cargo_package("my-app"),
    context,
)?;

// 3. update 中收到 DevSignal::Rebuilt 时交接窗口几何与业务会话状态并退出重执
Message::Dev(DevSignal::Rebuilt) => {
    nana_ui_dev::request_relaunch(DevHandoff { /* 窗口几何 */ }.with_state(&self.session));
    RuntimeProgramUpdate { exit: true, ..Default::default() }
}
```

此外，若使用 `.vue` 方言（Rust 脚本）编写视图且仅修改了静态文案，可在 `build.rs` 中开启 `Compiler::new("::nana_ui::runtime").hot(debug)` 并在 dev 入口使用 `watch_templates`（`templates` feature），即可在不重编译、不重启进程的情况下通过 `TemplateText::apply()` 实时替换窗口文字。

## 结合无头 Agent 会话实现免开窗验证

为 `dev` 二进制同时开启 `headless` feature（引入 `nana-ui-devtools` 的 `runtime-agent` 层），即可让同一个 `dev` 可执行文件兼具「正常桌面窗口」与「无头 JSON 自动化验证」两种模式——每次重编译后无需人工重新点击进入深层页面，直接通过管道发送 JSON 指令即可完成点击、无障碍树检查与离屏截图：

```bash
cargo build -p my-app --bin dev --features dev,headless

printf '%s\n' \
  '{"id":1,"cmd":"a11y"}' \
  '{"id":2,"cmd":"screenshot","path":"target/agent/after.png"}' \
  '{"id":3,"cmd":"click","agent_path":"open"}' \
  | ./target/debug/dev --stdio --width 320 --height 200 --theme dark
```
