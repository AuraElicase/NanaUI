# 两阶段启动与原生 Splash 合同

本文规定 NanaUI 的两阶段启动（Issue #225）：在不引入第二套 UI 引擎的前提下，将**原生首屏反馈（Early Splash）**与**完整界面就绪（UiReady / 画面接管）**解耦，消除应用初始化与着色器编译期间的空白窗口等待。

## 两个阶段与核心边界

1. **阶段一：Early Splash（原生启动窗）**。当 GPU 设备、管线与应用业务状态都尚未就绪时，宿主率先显示一个独立、无激活的小型原生 Splash 窗口。Logo 可直接编译进二进制，也可从 `early-splash` 资源包读取一次；呼吸或旋转等动画直接交由操作系统合成器（macOS Render Server / Windows DWM）推进，本进程零定时器、零逐帧回调（`SplashWork::animation_submissions` 恒为 `1`）。
2. **阶段二：`UiReady` 与画面接管（Takeover）**。当窗口、Surface、GPU 设备、`SceneWgpuPainter` 与文本系统全部就绪，能够挂载、布局并绘制普通文档时，宿主进入 `UiReady` 并调用 `RuntimeProgram::initialize`。应用在此挂载加载页或主界面并声明接管策略；**宿主必须确认接管内容的首帧已经成功提交给系统合成器后，才会撤下原生 Splash**。

::: tip 能力交接点 vs 画面交接点
`UiReady` 只代表「UI 渲染能力已就绪」，不代表漫长的业务加载已完成。如果不配置 `early_splash`，宿主不会创建任何额外图层或原生窗口，行为与传统单阶段启动完全一致。
:::

## 快速接入

```rust
static LOGO: &[u8] = include_bytes!("../assets/logo.png");

NanaApplication::builder(identity)
    .early_splash(
        SplashSpec::new(SplashLogo::png(LOGO))
            .with_logo_size(128.0, 128.0)
            .with_animation(SplashAnimation::Pulse),
    )
    .run::<RuntimeApplication<App>>(WindowDescriptor::new("App"))
```

不经过 `NanaApplication::builder` 的宿主（如 Vue 宿主）可使用 `nana_ui::with_startup(StartupOptions::default().with_splash(spec), || run_runtime::<P>(settings))`。完整可运行演示与验收探针见 `crates/nana-ui/examples/startup-splash.rs`（传入 `--probe` 可在交接完成后输出结构化 JSON 计时记录并校验合同）。

## 第一阶段：Early Splash 配置与平台实现

`SplashSpec` 包含以下四个配置项（此阶段不提供文字排版、进度条、脚本或自定义着色器；如需展示产品名称请直接合入 Logo 图片）：

| 字段 | 说明 |
| --- | --- |
| `logo` | `SplashLogo::png(&'static [u8])`（内嵌静态 PNG）或 `SplashLogo::packaged("nana://res/…")`（从 `early-splash` 资源包读取）。编码大小上限 `1 MiB`，最长边上限 `1024 px`，解码后像素上限 `4 MiB`（启动前先校验 PNG 头，超限直接拒绝而不解码）。 |
| `logo_size` | 独立 Splash 窗口与 Logo 内容框的逻辑像素尺寸（Logo 按比例缩放居中；独立 Splash 窗口不进任务栏、不抢焦点，仅跟随主窗口中心点与 DPI）。 |
| `background` | `System`（取默认主题调色板在当前系统明暗下的背景色）、`Color(..)` 或 `Transparent`（仅画 Logo）。若应用自身主题不跟随系统明暗（例如 Vue 宿主默认 `Light`），请显式指定 `Color(..)` 以避免交接时底色跳变。 |
| `animation` | `None`、`FadeIn`（默认，淡入一次后保持）、`Pulse`（呼吸脉动）、`Rotate`（持续旋转）。 |

### 跨平台原生实现与交接方式

| 平台 | 原生 Splash 实现 | 系统合成器动画 | 首帧无闪烁交接机制 |
| --- | --- | --- | --- |
| **macOS** | 独立无标题栏、透明背景、忽略鼠标事件的 `NSWindow` + `CALayer` | `CABasicAnimation`（由 Render Server 推进） | 主窗口在 Splash 期间以 `alphaValue = 0` 保持（可排版但不可见、不收指针）；接管目标帧 `present` 后将主窗口 `alphaValue` 恢复为 `1` 并 `orderOut` 移除 Splash |
| **Windows** | 独立无激活 `WS_POPUP` 窗口上的 DirectComposition 视觉树（由短生命周期 D3D11 设备上传一次位图，交接后立即释放） | `IDCompositionAnimation`（由 DWM 推进） | 主窗口在 Splash 期间通过 `DWMWA_CLOAK` 隐身；接管目标帧 `present` 并等待 `on_submitted_work_done` 完成后，取消 Cloak → `DwmFlush()` → 移除 Splash 视觉并释放 D3D11/DComp |
| **Linux 及其他** | 不显示原生 Splash，直接上报 `Skipped(PlatformUnsupported)` | — | `UiReady` 与接管 ticket 合同照常工作，应用代码无需分支判断 |

实际生效结果记录在 `StartupStatus::splash`（`SplashOutcome`）中，绝不把申请值当作结果：`Shown { animation: Applied(..) | Static { requested, reason } }`（当系统开启“减少动态效果”或合成器拒绝动画时自动降级为静态 Logo 并注明 `ReducedMotion` / `NativeAnimationFailed`）、`Skipped(..)`（未配置、隐藏启动 `visible = false`、嵌入式宿主或平台不支持）或 `Failed(Logo(..) | Package(..) | Native(..))`（Logo 损坏或超限时不阻断应用启动，仅跳过 Splash）。

### 从 `early-splash` 资源包读取 Logo

启用 `nana-ui` 的 `packaged-resources` feature 并配置 `resource_packs` 后，可使用 `SplashLogo::packaged("nana://res/splash/logo.png")` 从 `early-splash` 类别的资源包读取 Logo（耗时记录在 `StartupWork::splash_logo_read`，本机实测 71 KB Logo 仅需约 `0.25 ms`）：

- 路径通过已验签的 `package.json` 最长前缀路由到唯一 pack；该 pack 必须属于不加密的 `early-splash` 类别（否则拒绝打开并报 `NotEarlySplash`，全程不调用 `KeyProvider`）。
- 读取器读完 4 KiB 头部即检查文件总大小（超过 `4 MiB` 报 `EarlySplashTooLarge`），查 TOC 确认条目不超过 `1 MiB` 后逐块校验哈希并检查 PNG 头（开发期从 `target/` 运行时自动回退读取 `loose_root` 下的同名文件）。

## 第二阶段：`UiReady` 与画面接管（Takeover）

当 `RuntimeProgram::initialize`（或 `ApplicationState::initialize` + `build`）被调用时，即到达 `UiReady`。此时 `initialize` 只需挂载首屏视图（加载页或主界面）、将耗时业务任务派发给后台线程并立即返回。

`RuntimeProgram::startup_takeover()` 决定何时撤下原生 Splash：

- **`Immediate`（默认）**：`initialize` 构建的主窗口文档即为接管画面（当配置了 Splash 时，`initialize` 返回的初始消息会按 `2 ms / 64 条` 分批处理完毕后再发起接管，确保撤下 Logo 的那一帧已包含初始消息的效果）。
- **`Deferred`（延迟接管）**：保持显示 Splash（或保持加载态），直到应用显式调用 `context.startup().take_over(ticket)`：

```rust
let startup = context.startup();
let ticket = startup.status().ticket.expect("UiReady 之后即分配 ticket");
startup.take_over(ticket)?;          // 可从任意线程调用
startup.cancel_takeover(ticket)?;    // 撤回接管请求并使旧 ticket 作废
```

- **代际票据与目标帧确认**：调用 `cancel_takeover` 会使旧 `ticket` 作废（若接管帧已 present 则返回 `AlreadyHandedOff`；迟到的后台任务持旧票据调用会收到 `StaleTicket`）。`take_over` 会记录主窗口当前的 `flush` 序号——**只有在此之后完成 `flush` 且真正 `present` 成功的主窗口帧才算完成交接**（跳过的帧、失败的帧或设备重建前的旧帧均不算数）。
- **Vue / JS 投影接口**：前端通过 `Nana.startup` 访问同一套状态机（`Nana.startup.deferTakeover()`、`Nana.startup.state`、`Nana.startup.takeOver()`、`Nana.startup.onChange(fn)`）。

## 启动线程并行化与着色器编译加速

独立宿主（`run_runtime`）在窗口线程创建窗口与 Surface 后，立即将 GPU 适配器选择、设备请求与 `SceneWgpuPainter` 的管线编译放到后台的 `nana-startup-gpu` 线程执行，同时在主线程挂载原生 Splash——因此 **Splash 的显示与 GPU 初始化完全并行重叠**。

在 `nana-startup-gpu` 线程内部：
1. 若请求了合成后端（`CompositionCapable`），直接复用建窗前 DX12 合成探测已枚举出的适配器列表，省去第二次重复枚举（耗时从约 `0.74 s` 降至 `0.08 s`）；没有探测时在此枚举一次。在这份列表上按 `StartupOptions::adapter` 选卡，规则见 [GPU · 适配器选择](gpu.md#适配器选择)；
2. `SceneWgpuPainter` 的各大管线家族（quad、mesh/path、icon、text、HostTexture、backdrop）在 scoped 线程上并行编译，总构建耗时取决于最慢的单个家族（quad）而非全部累加；仅用于测试回读的 motion 求值管线不在产品启动路径中编译。
3. macOS 上唯一较慢的 Dock 图标重采样与 PNG 编码放在独立的 `nana-startup-icons` 线程异步完成，不阻塞 `UiReady`。

### DX12 着色器编译器

在 Windows DX12 下，`SceneWgpuPainter` 的启动构建耗时几乎全部花在着色器编译上。**DXC 比系统自带的 FXC 快约一倍**（实测 RTX 5060 Ti 上 release 构建的 painter 编译从 FXC 的 `2.24–2.39 s` 降至 DXC 的 `1.18–1.79 s`，debug 构建从 `4.73–7.77 s` 降至 `1.61–1.98 s`）：

- 当可执行文件（`.exe`）同目录下存在 x64 的 `dxcompiler.dll`（建议 DXC `v1.8.2502` 或更新版本，来自 [DirectXShaderCompiler releases](https://github.com/microsoft/DirectXShaderCompiler/releases) 或 Windows SDK `10.0.26100`，无需附带 `dxil.dll`）时，宿主会按完整绝对路径优先加载它（不受 `PATH` 或当前工作目录污染）；
- 未携带 `dxcompiler.dll` 时自动回退到系统内置的 FXC，也可通过环境变量 `WGPU_DX12_COMPILER`（`fxc` / `dxc` / `staticdxc` / `auto`）显式指定。

## 时间线观测指标

`StartupStatus::timeline` 记录从宿主入口（`run_runtime`）起算的六个关键里程碑：`splash_committed` → `ui_ready` → `takeover_requested` → `first_frame_submitted` → `handoff_completed` → `splash_released`；`StartupStatus::work` 与 `nana_diagnostics::framework::host`（`STARTUP_PHASE`、`SPLASH_OUTCOME`、`host.startup.device_request`、`host.startup.painter_build` 等）同步记录事件线程最长单次阻塞、设备请求耗时、painter 编译耗时与存活原生资源数（交接完成后 `SplashWork::live_resources` 严格归零）。
