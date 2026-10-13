---
title: 窗口与平台
---

# 窗口与平台:中立合同、原生边界与回退语义

这一章沿「谁拥有窗口」这条线走完最后一段:**`nana-ui-platform` 的平台中立窗口合同长什么样;`nana-window` 在原生一侧管句柄、材质、chrome、缩放与全屏;材质/合成/阴影失败时的回退语义为什么是「报告实况」而不是重试;原生句柄为什么到不了普通控件;`WebSurface` 的 headless 页面到底是什么**。读完你能搞懂一个窗口从 `WindowDescriptor` 到原生 HWND/NSWindow 的完整合同面,以及每一层「申请 → 实际应用」的记账。它承接 [GPU 与帧生命周期](/learn/gpu-frame)的 Surface 所有权结论。

## 职责与边界

- **`nana-ui-platform` 是合同层**:crate 文档自述 "Platform input / IME / clipboard abstraction"(`crates/nana-ui-platform/src/lib.rs:1`),窗口部分再导出 `WindowDescriptor`/`WindowEvent`/`WindowCommand`/`DisplayInfo` 等纯数据合同(`lib.rs:42`–49),并转发 `nana-ui-input` 的规范输入合同("The canonical input contract, owned by `nana-ui-input`",`lib.rs:156`)。它没有任何平台句柄类型——文档直言:"No platform type crosses this boundary"(`crates/nana-ui-platform/src/window.rs:494`)。
- **`nana-window` 是原生执行层**:"Platform-owned native window support for Nana applications"(`crates/nana-window/src/lib.rs:1`)。原生句柄(raw window handle)、DWM/DirectComposition、Mica/Acrylic/Vibrancy 材质、无框 chrome、实尺寸移动、underlay 伴生窗口、headless 网页都在这里,并且**只**在这里。
- **所有权表**:应用/宿主拥有 Window 与 Surface(AGENTS.md);`nana-window` 的每个函数都拿现成的 `impl HasWindowHandle` 借用窗口,不创建、不持有;控件(`PathField`、菜单、对话框请求方)拿不到句柄——需要句柄的操作(装菜单、开文件对话框)由宿主代执行,结果走事件回来。
- **边界之外**:呈现目标(合成路径)是 GPU/呈现层的事,见 [GPU 与帧生命周期](/learn/gpu-frame);`WebSurface` 不是 UI——AGENTS.md 规定 "Headless `WebSurface` pages are host content producers (frames to an application sink), not UI",也没有 WebView 产品壳。
- **实验位面收得很紧**:`PlatformCapabilities` 与 `SurfacePhase{Pending, Ready, Destroyed}` 都藏在 `experimental-android` feature 之后(`lib.rs:59`/`:141`),且能力表注释先声明红线:"This is not a second product paint kernel: the host still paints Runtime / UiScene through `SceneWgpuPainter`"(`lib.rs:87`)——`android_mvp()` 只报告 Surface(Vulkan)、V8、槽位控件与输入等已接线项,`desktop_shell` 明确为 false;配套测试断言能力表不虚报未接线服务(`lib.rs:111`)。

## 依赖与邻居

层级:core(L1)→ window(L2)→ platform(L3)→ runtime(L5)→ scene(L6)→ nana-ui(L7)。注意方向:`nana-window` 与 `nana-ui-platform` 都在 runtime 之下,普通控件编译期就够不到它们;`nana-ui`(宿主适配)同时引用两者把命令翻译成原生调用。

| 邻居 | 关系 |
| --- | --- |
| `nana-ui-input` | 规范输入合同由 platform 再导出(`lib.rs:156`),宿主把原生输入降格进去 |
| `nana-ui` | `WindowService`/`WindowHandle` 把平台命令排队回窗口线程;呈现解析 `ResolvedWindowPresentation` 落在这层 |
| `nana-gpu` | 呈现路径协商(合成、alpha 模式)与 `SurfacePresentationPolicy` 相接,见 [GPU 与帧生命周期](/learn/gpu-frame) |
| `nana-diagnostics` | 呈现/回退只出诊断标签(`SurfaceTargetFallback::label`),不格式化、不落 I/O(AGENTS.md 帧路径合同) |
| winit | `nana-window` 在原生 API 之上、winit 之下补平台缺口(无框样式守卫、layered 窗口整平、无激活显示) |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `WindowId` | `crates/nana-ui-platform/src/window.rs:5` | `pub u64`,`PRIMARY = 0`;平台后端的原生 ID 保持私有并映射到它 |
| `WindowDescriptor` | `crates/nana-ui-platform/src/window.rs:429` | 窗口申请单:尺寸/位置/透明/阴影/role/surface 等 |
| `WindowEvent` | `crates/nana-ui-platform/src/window.rs:114` | 结果事件:Ready/Resized/ModeChanged/FileDialogCompleted… |
| `WindowCommand` | `crates/nana-ui-platform/src/window.rs:684` | 宿主可执行命令:Move/SetFullscreen/OpenFileDialog… |
| `WindowGeometry` | `crates/nana-ui-platform/src/window.rs:12` | 物理/逻辑位置尺寸 + scale_factor + maximized |
| `DisplayInfo` / `DisplayId` | `crates/nana-ui-platform/src/window.rs:214` / `:210` | 显示器枚举与跨枚举稳定身份 |
| `WindowSurfacePreference` | `crates/nana-ui-platform/src/window.rs:496` | 每窗口呈现路径申请:Auto/NativeWindow/Composition/RequireComposition |
| `MousePassthroughMode` | `crates/nana-ui-platform/src/window.rs:653` | Off/Passthrough/Forward 三态穿透策略 |
| `MaterialEffect` / `MaterialOutcome` | `crates/nana-window/src/material.rs:36` / `:121` | 材质申请(含中文 label)与实际应用结果(含回退原因) |
| `WindowIcon` | `crates/nana-ui-platform/src/window.rs:311` | 非预乘 32 位 RGBA 图标申请(全出血品牌标默认) |
| `WindowLevel` / `SystemAppearance` | `crates/nana-ui-platform/src/window.rs:261` / `:284` | 置顶层级三档;浅色/深色外观 |
| `FullscreenRequest` / `WindowModeState` | `crates/nana-ui-platform/src/window.rs:254` / `:269` | 全屏申请与实际生效的全屏/层级/显示器状态 |
| `NonClientRenderingStrategy` | `crates/nana-window/src/chrome.rs:71` | StripFrameStyles / SuppressNonClientRendering |
| `ResolvedWindowPresentation` | `crates/nana-ui/src/presentation.rs:343` | 一个窗口的最终呈现:材质+alpha+目标+chrome+阴影 |
| `WindowHandle` | `crates/nana-ui/src/window_service.rs:271` | 可跨线程的窗口句柄:命令排队回窗口线程 |
| `Underlay` | `crates/nana-window/src/underlay.rs:42` | 贴在父窗口之下的伴生原生窗口绑定 |
| `WebSurface` / `WebFrame` | `crates/nana-window/src/web_surface.rs` | headless 网页引擎实例与其捕获帧 |
| `NativeSplash` / `SplashHandoff` | `crates/nana-window/src/splash.rs` | 原生启动帧与首帧交接 |
| `SkipTaskbarError` | `crates/nana-window/src/lib.rs:80` | `Unsupported` / `Failed`:任务栏隐藏的结果语义 |
| `LiveSizeMove` | `crates/nana-window/src/size_move.rs` | 原生实尺寸移动/缩放循环期间的伴随状态 |

## 内部结构

### 中立合同:WindowDescriptor / WindowEvent / WindowCommand

合同的核心设计是**申请与结果分离**:你交 `WindowDescriptor`,听 `WindowEvent`,发 `WindowCommand`;三者在 `crates/nana-ui-platform/src/window.rs` 一文件内闭环。描述符的缺省值也值得知道:`initial_size (1200.0, 800.0)`、`minimum_size (760.0, 520.0)`、`shadow: WindowShadow::Auto`(`window.rs:535`–541)。几个字段直接决定行为:`visible` "applied only after document initialization succeeds"(`window.rs:442`,初始化失败窗口不闪现);`fullscreen` 的语义同样是「尽力而为、结果走事件」——"Enter fullscreen when the window is first shown. If the display is no longer connected the window opens without fullscreen"(`window.rs:446`);`persist_key` 的注释解释了为什么存在——"Window ids are not stable across process restarts; this key is"(`window.rs:457`);`tag: Option<Arc<str>>` 是应用自选的窗口类别,对宿主不透明,"service-allocated ids never have to be matched to requests by order"(`window.rs:464`)。事件侧同样只有「实际发生了什么」:`ModeChanged` 文档规定在 Ready 之后、每次全屏/层级请求之后、以及平台报告变化时投递,"only when it differs from the previous delivery"(`window.rs:201`)——它是全屏/置顶状态的权威;`PointerPresenceChanged` 只报悬停的鼠标/笔,触摸不算,且原生拖拽产生的 leave 要扣到指针重新上报(`window.rs:182`);`FileDialogCompleted` 连取消也算完成("including a cancel — arrives through...",`window.rs:775`)。显示器身份 `DisplayId(pub u128)` "Equal across enumerations within one process; only macOS keeps it across reconnects"(`window.rs:210`);`logical_bounds` 用单一像素栅格推导,"so displays with different scale factors never overlap"(`window.rs:230`)。上一会话持久化的位置经 `clamp_position_to_displays`(`window.rs:64`)处理:与任何显示器相交就原样通过,否则夹进最近显示器——重启后拔了显示器不会把窗口开进虚空。

### nana-window:原生句柄、材质与 chrome

**材质**(`crates/nana-window/src/material.rs`)。`MaterialEffect{Solid,Transparent,Vibrancy,Mica,Acrylic}`(`material.rs:36`,`label()` 给中文标签),`wants_transparent_surface()` 即非 Solid(`material.rs:55`)。能力探测 `platform_material_support()`:macOS → `Vibrancy`,Windows → `MicaAcrylic`,其余 `None`(`material.rs:96`);宿主路径 `hosted_platform_material_support()` 的注释解释了一个 macOS 特例:"CAMetalLayer covers the AppKit visual-effect view, so `apply_hosted_system_material` always reports `MaterialFallback::NativeMaterialUnavailable`"(`material.rs:112`)。

**chrome**(`crates/nana-window/src/chrome.rs`)。透明客户区窗口的死结在文件头讲透:DWM 会为任何它认为带框的 HWND 在客户区**下面**画标题栏与投影;winit 的 undecorated 窗口保留全部三个样式位(WS_CAPTION/WS_THICKFRAME/WS_SYSMENU,`chrome.rs:5`–11),只靠 WM_NCCALCSIZE 把客户区盖上去——不透明客户区盖得住,透明的会透出来(`chrome.rs:13`)。两条出路各有代价:`StripFrameStyles`(默认)拿掉样式位,同时放弃 Aero Snap、Snap Layouts、Alt+Space 与系统窗口动画;`SuppressNonClientRendering` 保留样式位、请 DWM 别画非客户区,但 "Which one is correct is a measurement, not a deduction: `DwmSetWindowAttribute` reports success whether or not the policy has the intended effect"(`chrome.rs:71`,可用环境变量 `NANA_WINDOWS_NC_STRATEGY=suppress` 切换,`chrome.rs:86`)。样式守卫用 Win32 子类拦截 `WM_STYLECHANGING`:"A framework must not take `WM_STYLECHANGING` away from the rest of the system",所以在 `DefSubclassProc` **之后**应用掩码,让它成为最终意见(`chrome.rs:1364`)。此外 `set_present_transaction`(`chrome.rs:256`,macOS 把 Metal layer 的呈现钉进 Core Animation 事务)、`place_native_window_controls`(`chrome.rs:726`,红绿灯按钮对齐任意矩形)、`LiveFrameResize`/`LiveFrameMove`/`FrameGrab`(`chrome.rs:313`/`:471`/`:485`,指针驱动的帧缩放/移动)。

**underlay**(`crates/nana-window/src/underlay.rs`):`WindowRole::Underlay` 的实现体。"Glued directly beneath its parent... never taking input or activation"(`window.rs:302`);与影子伴生窗不同,它是**普通**顶层窗口、有自己的 Surface 与标题,"screen-capture software can capture it on its own — the point of splitting one visual window into two native ones"(`underlay.rs:1`)。几何权威在父窗口:"The parent's frame is the only authority... the host calls `Underlay::sync` on every geometry, mode, focus or visibility change of the parent — not per frame: on macOS it re-orders the window, which is a window-server round trip"(`underlay.rs:13`)。

### 缩放与全屏

缩放的权威表达是 `WindowGeometry{physical_position: Option, physical_size, logical_position: Option, logical_size, scale_factor, maximized}`(`window.rs:12`):物理与逻辑**同时**携带,调用方按需取用,不做隐式换算。指针命中换算在 `nana-window/src/pointer.rs`:统一入口吃 `(window, scale_factor, logical_size)`,`finite_scale` 把非有限值拉回 1.0(`pointer.rs:40`),避免脏缩放系数把指针映射到 NaN。全屏:`FullscreenMode{Borderless(默认), Simple}`——"Simple: macOS fullscreen without a separate Space"(`window.rs:245`),别处一律 Borderless;`SetFullscreen` 命令的文档写明结果语义:"A display that is not connected leaves the window unchanged; `WindowHandle::set_fullscreen` reports that error"(`window.rs:715`),而宿主路由处再补一句:"Batched commands have no reply; the outcome is `ModeChanged`"(`crates/nana-ui/src/scene_host/windows.rs:80`)——请求没有回执,以事件为准。

- **任务栏与图标**:`SetSkipTaskbar`(`window.rs:750`)的结果语义同上一条:Windows 上 "a hidden entry stays hidden when the window is shown again or Explorer restarts"(`lib.rs:102`),其余平台 `SkipTaskbarError::Unsupported`。图标走 `WindowIcon`("Unpremultiplied 32-bit RGBA... By default the icon is treated as a full-bleed brand mark... Artwork that already carries platform margins opts out via `exact_pixels`",`window.rs:311`);解析顺序在 `resolve_window_icon`:"Per-window override, else the process-wide registered icon"(`window.rs:424`),进程级注册用 `register_application_icon`(`window.rs:410`);宿主侧再有一条优先级:`SetIcon` 处 "The program's icon wins over one the startup is still rendering from the descriptor"(`scene_host/windows.rs:153`)。
- **内容纵横比**:`SetContentAspectRatio`(`window.rs:735`)请求后,"A windowed window is resized to `aspect_window_size` of its current size at once; a maximized or fullscreen one when it returns to windowed"——最大/全屏窗口不变形,回到窗口态才落。`aspect_window_size`(`window.rs:815`)"keeps the area of `size`, so switching between ratios and back returns the same size";`aspect_minimum_size`(`window.rs:803`)则 "One edge keeps its minimum and the other grows, so a landscape minimum forces portrait content tall"——最小值是硬边界,面积守恒服从它。

### 回退语义:申请 → 实际应用

这一层最系统的设计:**每个「申请」都配一个「实际应用了什么」的记录,失败不重试、不假装成功**。三处实例:

- **材质**:`apply_system_material(window, requested, appearance, fallback_color) -> MaterialOutcome`(`material.rs:176`)。失败落回实色并携带原因:`MaterialFallback::{PlatformDoesNotProvideNativeMaterial("当前设备不支持透明窗口效果"), NativeMaterialUnavailable("透明效果不可用,已使用实色背景")}`(`material.rs:61`)。AGENTS.md 合同 "Native material failure returns the actual applied outcome or an explicit fallback" 落在这里。
- **呈现目标**:`ResolvedWindowPresentation`(`crates/nana-ui/src/presentation.rs:343`)把材质结果、表面 alpha 模式、呈现目标与 chrome **一次性**解析:"The demotion and the chrome derivation happen together here, on purpose: there is no way to obtain an effective material without also obtaining the chrome that matches it"(`presentation.rs:363`)。合成目标失败给 `SurfaceTargetFallback::{BackendUnavailable, TargetUnavailable}`(`presentation.rs:246`),中文标签只用于诊断,"a product surface that wants to say something about transparency says it from `MaterialOutcome::status_label`"(`presentation.rs:263`)。`Composition` 申请失败只是被告知,`RequireComposition` 才拒开窗:"would rather not run at all than run wrong"(`window.rs:511`)。窗口关闭后 `closed()` 返回普通不透明呈现,"Reporting the last live window's state here would be a second, stale authority"(`presentation.rs:395`)。阴影字段独立记账,`Pending` 直到宿主应用到已显示窗口(`presentation.rs:410`)。
- **穿透与任务栏**:`skip_taskbar` 的结果经 `SkipTaskbarChanged` 报告,"a failed request leaves the previous value"(`window.rs:141`);Windows 之外返回 `Unsupported` 而不是假装成功(macOS 理由:"the Dock shows the application, not individual windows",`crates/nana-window/src/lib.rs:102`)。

### 句柄为什么不出控件

三层防线。**编译期**:`nana-window`/`nana-ui-platform` 在依赖图上位于 runtime(L5)之下,普通控件(住在 runtime 及以上)根本引用不到句柄类型;`MousePassthroughMode` 的文档点破动机:"Host-owned mouse-passthrough policy. Widgets never see the native handle."(`window.rs:651`)。**运行期**:需要句柄的操作由宿主代跑——`SetMenuBar` 注释:"The menu is described by the application but installed by the host, because on Windows it belongs to a window and controls never reach a window handle"(`window.rs:765`);`OpenFileDialog` 同理("The host opens it because the dialog needs the parent window handle, which controls never reach",`window.rs:775`)。**借用期**:`nana-ui` 的 `WindowHandle`(`crates/nana-ui/src/window_service.rs:271`)只是 `(id, generation, service)` 的可克隆值,所有操作经 `sync_channel(1024)` 排队回窗口线程(`window_service.rs:203`);唯一的原生接触面 `with_native_handle` 在**窗口线程上**回调,"The borrowed native handle is valid only during the callback; do not retain raw pointers or use them after return"(`window_service.rs:279`)。宿主自己借句柄也一样短暂:`scene_host/windows.rs:143` 里 `host.surface.window().as_ref()` 传给 `set_native_window_controls_visible` 当场用完。

### 指针、命中与穿透边界

指针位置换算是窗口层最容易踩 NaN 的地方,`crates/nana-window/src/pointer.rs` 的做法是把入口收成一个:`pointer_in_client_area(window, scale_factor, logical_size, position)` 统一吃逻辑坐标与缩放系数,`finite_scale` 把非有限缩放拉回 1.0 再参与映射——脏 scale 不会把指针打到 NaN 坐标上。窗口级鼠标穿透由 `MousePassthroughMode`(`window.rs:653`)三分:`Off`(默认)、`Passthrough`(整窗原生命中关闭,注释警告 "Overlay does not receive pointer events and cannot recover itself"——罩层自己救不回自己)、`Forward`(指针在透明空白处时保持原生命中关闭,宿主采样全局指针,遇到不透明或可交互内容再恢复)。Windows 上穿透的实现细节解释了 `settle_layered_window`(`crates/nana-window/src/lib.rs:141`)为什么存在:"Windows turns a window click-through by making it `WS_EX_LAYERED | WS_EX_TRANSPARENT`, and a layered window without layering attributes is not composed: a DirectComposition window keeps presenting into nothing"——给这类窗口补一个不透明的常量 alpha(逐像素 alpha 不变),别人设置的属性不动。焦点侧还有 `show_without_activation`(`lib.rs:128`):"winit's `set_visible(true)` makes a macOS window key",所以创建后显示的窗口要显式走它才能兑现 `focus_on_show = false`。

### 菜单、文件对话框与启动画面

三个「控件发起、宿主执行」的原生服务,全部遵循句柄不出控件的合同:

- **菜单**:`SetMenuBar`(`window.rs:765`)由宿主安装,选择结果经 `take_menu_activations` 回流(`nana-window` 导出,macOS 原生菜单栏;其余平台由 `nana-ui-runtime` 的 AppTitleBar 控件承担,命令被显式忽略——见[架构:窗口](/architecture/window))。
- **文件对话框**:`OpenFileDialog`(`window.rs:775`)宿主代开(它需要父窗口句柄),结果走 `WindowEvent::FileDialogCompleted`/`FileDialogRejected`(`window.rs:124`/`:118`)立刻唤醒宿主;`PathField` 控件只发 `BrowseRequested`。取消不是错误。
- **启动画面**:`nana-window/src/splash.rs` 的 `NativeSplash`/`SplashSpec`/`SplashHandoff`(导出于 `lib.rs:67`)承载「原生启动帧 → 首帧 Ready」的交接,配合 `WindowDescriptor.visible` 的 "applied only after document initialization succeeds"(`window.rs:442`)消除白屏窗口。

### WebSurface:headless 页面是什么

`WebSurface`(`crates/nana-window/src/web_surface.rs`)是「平台网页引擎实例,永不进入宿主窗口的视图树」:"Its page is laid out at `WebSurfaceDesc::size` CSS pixels and captured at most `max_fps` times per second; each capture reaches the application's `WebFrameSink` on a background thread, so the UI thread never converts or queues pixels"(`web_surface.rs:3`)。帧是 CPU 侧 RGBA:"Tightly packed RGBA8, sRGB-encoded, alpha premultiplied in sRGB space"(`web_surface.rs:113`),序列号逐帧递增;边界常量 `MAX_WEB_SURFACE_EDGE = 4096`、`MAX_WEB_SURFACE_FPS = 60`(`web_surface.rs:29`–31),`scale` 夹在 0.25–4.0(`web_surface.rs:58`)。去向由应用决定——文档明说 "The application decides where frames go (a HostTexture slot, a compositor of its own)"(`web_surface.rs:8`),即生产者帧进[帧交换](/learn/gpu-frame),不构成第二 UI 树。`ShowWindow` 把同一页面挪进普通原生窗口做直接交互(登录、点设置),"cookies and page state stay, and frames keep flowing while it is open"(`web_surface.rs:11`)。慢消费者的语义写在 sink 类型上:"a slow sink drops later captures instead of queueing them"(`web_surface.rs:21`)。构建期支持查询 `web_surface_support` 区分「没编进去」与「运行期失败(旧 OS、缺 WebView2 Runtime)」(`web_surface.rs:128`)。

## 协作与数据流

一个窗口命令的完整旅程(以全屏为例):

1. 控件/应用调用 `WindowHandle` 方法 → `Request::Control(id, generation, control, reply)` 入队(`window_service.rs:336`),generation 不匹配即失效;
2. 宿主窗口线程 `scene_host/windows.rs` 路由 `RoutedWindowCommand::SetFullscreen` → `set_window_fullscreen`,无回执(`windows.rs:80`);
3. 平台层按 `FullscreenRequest{mode, display}` 落原生调用(Borderless/Simple 语义见上);
4. 状态变化经 `scene_host/display.rs` 投递 `WindowEvent::ModeChanged`(只在真的不同时,`display.rs:217`),进入程序事件循环。

材质与呈现的解析发生在窗口创建/Surface 绑定时:`WindowDescriptor.surface`(`window.rs:484`)给出每窗口偏好,宿主结合进程级 `GpuBackendPolicy` 与实际后端(`CompositionAvailability::for_backend`,`presentation.rs:210`)定目标,`apply_system_material` 出 `MaterialOutcome`,一起进 `ResolvedWindowPresentation`;`system_caption = true` 的窗口 chrome 记 `None`——"the host must not touch its frame, corners or stroke"(`presentation.rs:387`)。定制标题栏路径则由 `prepare_client_chrome` + 样式守卫维持帧不变量,客户区 8px 边缘缩放由 `FrameResizeEdge`/`resize_custom_frame`(`chrome.rs:224`)承接。

窗口的出生路径同样是一条申请→结果链:`WindowService::create_window(descriptor)`(`window_service.rs:224`)先校验描述符再向窗口线程发 `Open`;窗口线程 `scene_host/windows.rs:40` 打开窗口,失败投 `WindowEvent::OpenFailed`("no window is retained for this id",`window.rs:129`)——id 不留残骸;成功则等文档初始化完成后投 `Ready`(`window.rs:146`),`visible` 才在此后生效。显示器枚举 `displays()` 也在窗口线程上做(`window_service.rs:234`),换来的 `DisplayInfo` 列表支撑 `clamp_position_to_displays` 与 `FullscreenRequest.display` 的解析。窗口销毁侧,`discard_window` 与 `ResolvedWindowPresentation::closed()`(`presentation.rs:395`)对齐:后者返回普通不透明呈现,不再当旧状态的陈旧权威。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-platform/src/lib.rs` | 合同层入口:窗口再导出、输入合同转发 |
| `crates/nana-ui-platform/src/window.rs` | Descriptor/Event/Command/Geometry/Display/Role/Passthrough 全合同 |
| `crates/nana-ui-platform/src/window_shadow.rs` | 阴影合同(与呈现的 shadow 字段相接) |
| `crates/nana-window/src/lib.rs` | 原生层导出面、skip_taskbar、无激活显示、layered 整平 |
| `crates/nana-window/src/material.rs` | 材质效果/能力探测/回退结果 |
| `crates/nana-window/src/chrome.rs` | 无框样式、DWM 策略、守卫子类、按钮定位、实尺寸移动 |
| `crates/nana-window/src/underlay.rs` | Underlay 伴生窗口 |
| `crates/nana-window/src/pointer.rs` | 指针→客户区命中换算(`finite_scale` 防护) |
| `crates/nana-window/src/splash.rs` | 原生启动帧与 SplashHandoff |
| `crates/nana-window/src/menu.rs` / `file_dialog.rs` / `size_move.rs` | 菜单安装、文件对话框、实尺寸移动 |
| `crates/nana-window/src/web_surface.rs` | headless 网页:Desc/Command/Event/Frame |
| `crates/nana-window/src/platform/{windows,macos,fallback}.rs` | 按平台 cfg 分发的原生实现 |
| `crates/nana-ui/src/window_service.rs` | `WindowService`/`WindowHandle` 队列与原生借用 |
| `crates/nana-ui/src/presentation.rs` | `ResolvedWindowPresentation` 与呈现/目标/chrome 解析 |
| `crates/nana-ui/src/scene_host/windows.rs` | 宿主命令路由(窗口线程侧) |
| `crates/nana-ui/src/scene_host/display.rs` | 显示器/全屏状态变化投递 |

## 附:所有权速查

| 资产 | 所有者 | 借用者 |
| --- | --- | --- |
| 原生窗口(HWND/NSWindow)与 Surface | 应用/宿主 | `nana-window` 的函数按调用借用,不持有 |
| 原生句柄类型本身 | `nana-window`(及 raw-window-handle) | 控件编译期不可见;`WindowHandle::with_native_handle` 回调内有效 |
| 菜单/文件对话框执行 | 宿主(需要父句柄) | 控件只发意图(`BrowseRequested`/菜单描述) |
| 显示器枚举与几何权威 | 窗口线程(`WindowService::displays`) | `clamp_position_to_displays`/`FullscreenRequest` 消费 |
| 全屏/置顶生效状态 | 平台 → `WindowEvent::ModeChanged` | `WindowModeState` 只读镜像;`set_fullscreen` 的错误在请求侧报 |
| 材质实际结果 | `MaterialOutcome` → `ResolvedWindowPresentation` | 设置界面用 `status_label()` 呈现,不重试 |

## 延伸阅读

- [GPU 与帧生命周期](/learn/gpu-frame):Surface 所有权、合成路径与帧槽;
- [架构:窗口](/architecture/window):应用作者视角的材质/标题栏/对话框;
- [架构:GPU](/architecture/gpu)与[架构:帧](/architecture/frame);
- [工作原理](/reference/how-it-works)与[模块与分层](/learn/modules-and-layers)。
