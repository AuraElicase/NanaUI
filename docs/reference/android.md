# Android 实验宿主

Android（`platform/android`）是仓库内的**实验性 ARM64 宿主路径**，并非当前正式产品交付目标，也不是第二套产品渲染内核——请勿将其写入面向用户的跨平台承诺。

比它低一级的 HarmonyOS / OpenHarmony 编译档见 [OHOS](ohos.md)：只承诺能编译，没有仓库内宿主。

在 Android 宿主上，界面同样进入 `nana-ui-runtime`（`UiWorld`）与 `nana-ui-scene`（`UiScene`），并由同一个 `SceneWgpuPainter` 绘制；窗口与 Vulkan 图形设备由宿主的 Android `Activity` 持有（不调用桌面端的 `run_runtime`）。

## 窗口内容

整个 Surface 是一份 Runtime 文档。`GameActivity` 把 Surface 铺到状态栏、导航栏和软键盘下面；文档根节点铺主题背景色，并按这三者的 inset（`WindowInsets` 的 `systemBars | displayCutout` 与 `ime`）留出内边距，内容因此落在 Primary 区：上方是外观设置页，下方是控件条（`Button` / `Text` / `TextInput` / `Switch`）。Primary 之外的触摸留给系统。

外观页有两组分段控件：

- **主题**：浅色、深色、跟随系统。跟随系统读 `Configuration` 的 `uiMode` 夜间位，系统切换深色模式时经 `ConfigChanged` 更新。选中后安装对应的内置主题（`set_theme_tokens`）。
- **文字大小**：跟随系统、小、标准、大。跟随系统读 `Resources.getConfiguration().fontScale`；选中后经 `set_document_text_scale` 重排整份文档。

清单的 `configChanges` 含 `uiMode|fontScale|density`，这些变化不重建 Activity。

## Phase 2 平台合同（基于 `GameActivity`）

相较于早期的 `NativeActivity`，当前 Android 宿主基于 `android-activity` 的 `game-activity` feature 构建（`NanaActivity` 继承自 `GameActivity`，从而具备 `InputConnection` 与 `GameTextInput`）：

- **输入法（IME）**：组字更新映射为 `CompositionInput::Update`，提交映射为 `Commit`，删除上下文映射为 `DeleteSurrounding`；它们与触摸、按键事件一样，由文档的输入源盖上序列号后统一走 `AppContext::route_input`，不维护第二套 Android 文本状态机。`EditorInfo` 从当前聚焦的编辑器镜像 `password` 与 `multiline` 标志（`PlatformCapabilities::android_mvp().ime = true` 仅表示该通道已接通，不等同于桌面级完整验收）。
- **系统剪贴板**：通过 JNI 调用系统 `ClipboardManager` 实现 `ClipboardHost`，并接入 `default_shared_clipboard()` 的 Android 分支；文档的 `HostServices` 经 `read_shared_clipboard` / `write_shared_clipboard` 使用这一后端（`clipboard = true`）。
- **应用数据目录**：`app_data_dir` 在 Android 上直接返回沙箱内的 `Context.getFilesDir()`（忽略 `app_id` 参数）；由宿主自行调用 `FileStore::open` 后注入，框架不私自写盘。
- **无障碍（TalkBack）**：接入 `Click`、`Focus`、`SetValue`、`SetSelection` 动作投影后，`Button`、`Switch` 与 `TextInput` 以及外观页的分段选项可被激活（`TextInput` 的 `Click` 会请求焦点以拉起软键盘；滚动与虚拟列表投影留待后续阶段）。

## 交叉编译与 APK 打包

本机交叉编译与桌面端运行的 `nana-android-host --lib` 单元测试仅验证 Rust 侧的动作队列、输入/IME 映射与无障碍投影合同，**不能替代真机上的中日韩输入法候选框、系统剪贴板或 TalkBack 触摸浏览验收**。

```bash
# 1. 仅做 ARM64 交叉编译检查：依次 check nana-ui-core、nana-ui-platform、nana-ui --lib 与 nana-android-host（已在 CI android-arm64-cross 中自动执行）
./scripts/check-android-arm64.sh

# 2. 构建 dist 档并打包可安装的 APK（需配置 Android NDK 与 GameActivity 4.4.0 Gradle 包装，勿开启 prefab）
./scripts/check-android-arm64.sh --build --dist
./scripts/package-android-host-apk.sh
```

构建注意事项：
- 打包 APK 必须使用 `--dist` 配置：`dev` 构建会在 `.so` 中内嵌约 `390 MB` 的 DWARF 调试信息与 `46 MB` 符号表，而 `package-android-host-apk.sh` 对 stripped `.so` 设有 `60 MiB` 体积上限。APK 内的 `.so` 采用未压缩方式存储并配合 `extractNativeLibs="false"`，安装后不再解压出第二份副本。更多工程细节见 `platform/android/README.md`。

## 真机验收清单

在真机安装 APK 后需逐项核对以下行为（2026-09-16 已在 Galaxy Z Fold4 Android 16 上通过前四项真机验收，见 #83）：

| 验收项 | 期望行为 | 证据要求 |
| --- | --- | --- |
| **软键盘唤起与收起** | 点击控件条里的 `TextInput` 弹出软键盘，失焦后自动收起；控件条随 IME inset 抬升至键盘上方 | 真机或模拟器 |
| **系统栏避让** | 外观页顶端在状态栏下方，控件条底端在导航栏上方；两条系统栏下面是主题背景色 | 真机，含刘海 / 挖孔屏 |
| **外观页** | 浅色 / 深色立即换主题；跟随系统随系统深色模式切换；文字大小四档重排，跟随系统随系统字号变化 | 真机 |
| **CJK 输入法候选** | 拼音/笔画候选词选定后正确写入 Runtime `TextInput`（宿主暂未提供 `CursorAnchorInfo`，候选条停留在键盘顶端） | **必须使用真机输入法**，不得用 `adb shell input keyevent` 替代 |
| **系统剪贴板互通** | `TextInput` 内复制/粘贴与系统 `ClipboardManager` 互通 | 真机 |
| **TalkBack 读屏交互** | `Button` 激活计数、`Switch` 状态切换、`TextInput` 聚焦并进入编辑、外观页分段选项切换 | **必须由真人在真机上做 TalkBack 手势**（`adb` 注入的 tap 会绕过无障碍触摸浏览） |
| **滚动与虚拟列表** | 尚未接入 Android 侧滚动无障碍动作 | 留待后续阶段 |
