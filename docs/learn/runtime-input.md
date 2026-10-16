---
title: 输入链路
---

# 输入链路

输入事件从原生操作系统窗口（或 Vue 宿主、无头测试会话）到达保留树上的某个控件，中间经过两层严格分工的合同：**`nana-ui-input` 定义「规范事件与宿主服务长什么样」**，**`nana-ui-runtime` 的 `framework/input.rs` 决定「事件路由并分发到哪个节点头上」**。本章沿这条完整链路拆解：规范事件与序列号、有界端点与合并、`AppContext::route_input` 的五步校验与指针本地化、指针/滚轮/按键/输入法四条有序分发链、焦点与 roving focus、IME 与 `HostServices` 状态回写，以及无障碍增量投影。

## 职责与边界

- **`nana-ui-input`（规范输入合同）**：定义规范输入事件的形状、身份、单调序列号、有界收件箱端点（`InputEndpoint`）与宿主服务回调（`HostServices`），不含任何平台特定代码（`crates/nana-ui-input/src/lib.rs:5`）；`nana-ui-platform` 原样再导出整套合同。
- **`nana-ui-runtime`（唯一输入路由器）**：每个 `AppContext` 持有一份独立的路由状态（`crates/nana-ui-runtime/src/framework/input.rs:1`），是宿主输入事件进入保留世界的**唯一路径**；不同窗口绑定各自的输入源，绝不混用指针捕获、IME 焦点或光标状态。
- **宿主适配层（降维与回写）**：原生窗口在 `crates/nana-ui/src/scene_host/input.rs` 中将 winit 事件盖章推入端点；Vue 宿主在 `crates/nana-ui-vue/src/host/input_projection.rs:386` 中将 JS 采样转为 `CanonicalInputEvent` 后送入同一个 `route_input`。

## 依赖与邻居

| 层级 / Crate | 角色说明 |
| --- | --- |
| `nana-ui-input` | 定义规范事件、序列器、有界端点与 `HostServices` 合同 |
| `nana-ui-platform` | 原样再导出输入合同，供窗口层与宿主层使用 |
| `nana-ui-runtime` | 唯一路由实现（`route_input`）、四条分发链、焦点管理、IME 状态与无障碍增量投影 |
| `nana-ui` / `nana-ui-vue` | 将平台原生事件降维为规范事件，并在 drain 末尾将 `HostServices` 状态写回原生窗口 |
| `nana-text` | 文本编辑路由时提供 `TextShaper` 度量支持 |
| `nana-diagnostics` | 记录端点与路由高频指标（`runtime.input.*`，只走原子 `metric!` 而非逐帧事件） |

## 关键类型与数据

| 类型 | 所在文件与行号 | 作用说明 |
| --- | --- | --- |
| `CanonicalInputEvent` | `crates/nana-ui-input/src/canonical.rs:44` | 统一事件信封：源 ID、设备 ID、端点代际、序列号、时间戳与载荷 |
| `InputPayload` | `crates/nana-ui-input/src/canonical.rs:209` | 13 种规范载荷：指针、滚轮、按键、已提交文本、IME 组字、文件拖放、焦点、断连等 |
| `CommittedText` | `crates/nana-ui-input/src/canonical.rs:269` | 已提交文本载荷，携带产生该文本的按键序列号（用于抑制快捷键触发后的字符插入） |
| `CompositionInput` | `crates/nana-ui-input/src/canonical.rs:191` | IME 组字生命周期载荷（`Start` / `Update` / `Commit` / `End` / `DeleteSurrounding`） |
| `InputSequencer` | `crates/nana-ui-input/src/canonical.rs:333` | 为事件盖上严格递增序列号与动画时钟域时间戳，管理端点代际推进 |
| `InputEndpoint` | `crates/nana-ui-input/src/canonical.rs:410` | 有界单源收件箱队列，支持相邻指针移动与滚轮事件合并（coalesce） |
| `HostServices` | `crates/nana-ui-input/src/host_services.rs:97` | 光标样式、IME 文本输入上下文与系统剪贴板回调合同 |
| `InputRouteOutcome` / `InputRouteError` | `crates/nana-ui-runtime/src/framework/input.rs:36` / `:74` | 单事件路由结果与五类校验拒绝原因 |
| `KeyStroke` | `crates/nana-ui-runtime/src/framework/input/dispatch.rs:55` | 按键按下与 `CommittedText` 共用的键链分发载体 |
| `SourceEffects` | `crates/nana-ui-runtime/src/framework/input/effects.rs:43` | 光标与 IME 上下文的最新值槽位（仅在状态实际变化时通知宿主） |
| `InputState` / `SourceState` | `crates/nana-ui-runtime/src/framework/input.rs:247` / `:167` | 路由器全局状态与每输入源独立状态 |
| `HeadlessInput` | `crates/nana-ui-runtime/src/framework/input/headless.rs:19` | 无头测试与 Agent 会话使用的确定性输入源 |

## 内部结构

### 1. 规范事件：身份、序列号与有界合并端点

`canonical.rs`（`crates/nana-ui-input/src/canonical.rs:1`）立下三条硬规矩：宿主在边界处一次性降维原生消息；事件时间戳统一使用 Runtime 动画时钟域的纳秒数（使悬停节流、双击判定与动画处于同一时钟）；序列号在同一个端点代际（`EndpointGeneration`）内跨设备严格单调递增。

- **按键与文本抑制（`CommittedText`）**：按键载荷同时携带与键盘布局无关的物理键 `PhysicalKey` 和逻辑键 `LogicalKey`。`CommittedText`（`:269`）额外记录产生它的按键序列号——当某个按键已被 Runtime 作为快捷键、Tab 焦点移动或回车提交消费掉时，`SourceState::handled_key` 会记下该序列号，路由器随后收到携带相同序列号的 `CommittedText` 时会直接将其抑制丢弃（计入 `text_suppressed`），与浏览器对 `keydown.preventDefault()` 后跳过 `input` 事件的语义完全一致。
- **有界端点与安全合并（`InputEndpoint`）**：原生窗口端点容量为 `ENDPOINT_EVENTS = 1024`、`ENDPOINT_PAYLOAD_BYTES = 1 MiB`（`crates/nana-ui/src/scene_host/host_services.rs:22`）。队列满时将事件退回调用方触发先 drain 再重试，绝不静默丢弃按键、按钮或 IME 状态转移。`coalesce`（`canonical.rs:470`）仅合并相邻的 `PointerMove` 与 `Wheel`（滚轮增量求和、位置取最新），且仅当新事件的序列号严格递增、时间戳不回退时才允许合并，防止合并操作掩盖乱序错误。

### 2. 路由入口：五步校验与指针 ID 本地化

宿主先调用 `bind_input_source` 将输入源绑定到文档，随后通过全仓唯一的 `AppContext::route_input`（`crates/nana-ui-runtime/src/framework/input.rs:397`）或批量 `drain_input`（`:430`）路由事件。`route_validated`（`:508`）按固定顺序执行五步校验，任一步不满足即返回对应的 `InputRouteError`（`:74`）：

| 校验顺序与错误变体 | 拒绝触发条件 |
| --- | --- |
| 1. `UnknownSource` | 输入源尚未调用 `bind_input_source` 或已解绑 |
| 2. `StaleGeneration` | 事件携带的 `EndpointGeneration` 早于当前绑定代际 |
| 3. `Disconnected` | 设备已断开且该事件序列号晚于断连事件 |
| 4. `OutOfOrder` | 事件序列号没有严格大于该源上一条已处理事件 |
| 5. `TimestampRegression` | 事件时间戳小于该源上一条已处理事件 |

校验通过后，指针事件先经 `localize`（`:64`）将平台指针 ID 映射为 `AppContext` 本地指针 ID，再进入分发链；每个事件处理末尾立即调用 `flush_reactive()` 结算信号写入，并根据 `pending_work_revision` 判断是否标脏了帧工作（`InputRouteOutcome::invalidated_work`）。当没有新输入事件但滚动、布局或动画移动了界面元素时，宿主在帧后调用 `refresh_input_effects`（`:478`），在静止指针坐标下重新推导悬停（hover）与光标状态。

### 3. 四条有序分发链（`dispatch.rs`）

`crates/nana-ui-runtime/src/framework/input/dispatch.rs` 将事件分为四条优先级明确的责任链（前面的环节消费后，后面的环节不再响应）：

1. **指针链（`dispatch_pointer:88`）**：未捕获指针每次移动仅执行 **1 次**命中查询（`UiWorld::hit_test`），已捕获指针执行 **0 次**命中查询。按下（Down）依次经过：浮层外点击关闭（dismiss）→ 滚动条车道拦截 → 可达手柄 → `focus_node_in_place`（就地聚焦而不触发自动滚动，防止按下瞬间内容滚走导致释放落空）→ 文本编辑器与文档选区按下 → 各复合组件节点指针钩子（TreeView / AreaLayout / ScrubField / TimelineEditor / PropertyGrid / 图表 / Dock / SplitPane / Workspace 等）→ 通用 `press_pointer`。
2. **滚轮链（`dispatch_wheel:690`）**：行滚轮增量乘以 `LINE_SCROLL_EXTENT = 60.0`，沿命中节点向最近的可滚动祖先容器传递。
3. **按键与文本链（`dispatch_keystroke:800`）**：菜单栏按键路由（`AppMenuBar` 的 `Alt` / `F10` 与方向键）与模态阻断浮层 → 应用级按键策略 → 聚焦终端 / 富文本编辑器 / 纯文本编辑器（含 TreeView 行内重命名与 ScrubField 键入）→ Tab 顺序焦点遍历（`navigate_sequential_focus`）→ 分段控件与撤销/重做 → 剪贴板快捷键（经 `HostServices`）→ 聚焦控件自身按键 → 表格与 roving focus 方向键导航（`roving_focus_key`）→ 弹出菜单键盘触发（`keyboard_menu_key`）。在 `Ctrl` / `Cmd` 组合键下，`shortcut_key`（`framework/input.rs:214`）会将西里尔、希腊、希伯来等非拉丁键盘布局按物理键位映射回拉丁字母，使快捷键跨键盘布局一致生效。
4. **输入法组字链（`dispatch_composition:1252`）**：路由至当前拥有 IME 会话的可编辑字段（`Start` → `set_ime_preedit("")`，`Update` → 更新 preedit 覆盖层，`Commit` → 通过组件提交文本，`DeleteSurrounding` → 删除光标周围字符）。

### 4. `HostServices` 状态槽与无障碍增量投影（含虚拟节点）

- **`HostServices` 是最新状态槽而非请求队列**：`HostServices`（`crates/nana-ui-input/src/host_services.rs:97`）与 `SourceEffects`（`framework/input/effects.rs:43`）将鼠标光标与 IME `TextInputContext` 建模为「最新值槽位」——路由器只在状态实际改变时通知宿主，原生窗口的 `NativeWindowServices` 在每次 drain 末尾一次性同步到系统窗口，绝不会因为队列积压过期光标请求而阻塞输入。
- **增量无障碍投影与虚拟子节点**：`project_accessibility_delta`（`crates/nana-ui-runtime/src/world/accessibility.rs:630`）在每帧布局收敛后产出带墓碑（tombstone）的无障碍增量事务。对于 `TreeView` 和打开的菜单这类自绘单节点控件，投影器使用高两位标记的**虚拟节点 ID** 将每个可见行投影为父控件下的无障碍子节点：菜单行使用 `virtual_menu_item_id`（第 63 位为 `1`、第 62 位为 `0`），树行使用 `virtual_tree_item_id`（`0xC000_0000_0000_0000 | (tree << 16) | slot`，第 63 与 62 位均为 `1`，角色为 `AccessibilityRole::TreeItem`，携带 `level`、`expanded`、`selected`、`disabled` 与行几何矩形）。读屏软件发起的 `apply_accessibility_action`（`Click` / `Focus` / `Expand` / `Collapse`）先解码虚拟行 ID 再交由同一个组件逻辑处理，确保读屏操作与鼠标点击、键盘方向键产生完全一致的事件序列。

## 协作与数据流

一次原生窗口输入的完整闭环：

1. winit 原生窗口事件进入 `handle_window_event`（`crates/nana-ui/src/scene_host/input.rs:54`）；
2. `push_input`（`:475`）用该窗口源的 `InputSequencer` 盖上序列号与时间戳，推入 `InputEndpoint`（若队列满则先执行一次 drain 再重试）；
3. `drain_window_input`（`:509`）调用 `AppContext::route_input` 完成校验、指针本地化与四条链分发，事件回调产生的信号与树修改在末尾通过 `flush_reactive()` 结算；
4. 帧边界由 `RuntimeDocument::flush` 排空待处理系统工作（见 [保留树：UiWorld](/learn/runtime-tree)），宿主随后调用 `refresh_input_effects` 并在 `NativeWindowServices` 中一次性应用最新光标与 IME 状态。

## 代码地图

| 文件路径 | 核心内容 |
| --- | --- |
| `crates/nana-ui-input/src/canonical.rs` | `CanonicalInputEvent`、身份类型、`InputSequencer`、`InputEndpoint` 与相邻事件合并 |
| `crates/nana-ui-input/src/host_services.rs` | `HostServices` 合同、光标枚举、`TextInputContext` 与无头实现 |
| `crates/nana-ui-runtime/src/framework/input.rs` | 唯一路由器 `route_input`、五步校验、指针本地化、焦点路由与断连清理 |
| `crates/nana-ui-runtime/src/framework/input/dispatch.rs` | 指针、滚轮、按键与输入法组字四条分发链 |
| `crates/nana-ui-runtime/src/framework/input/effects.rs` | `SourceEffects` 光标与 IME 最新值槽位、8 ms 手柄探测节流 |
| `crates/nana-ui-runtime/src/framework/input/headless.rs` | `HeadlessInput` 无头确定性输入源 |
| `crates/nana-ui-runtime/src/world/hit_test.rs` | 增量命中索引与变换感知命中查询 |
| `crates/nana-ui-runtime/src/world/accessibility.rs` | 无障碍增量投影、墓碑与菜单/树虚拟行节点编码 |
| `crates/nana-ui/src/scene_host/input.rs` | 原生窗口事件入队、drain 与文档重绑清理 |
| `crates/nana-ui/src/scene_host/host_services.rs` | 原生窗口的 `HostServices` 实现与端点容量常量 |

## 延伸阅读

- [保留树：UiWorld](/learn/runtime-tree)：事件回调写入的 `MutationQueue` 如何提交，帧内系统工作如何排空
- [布局引擎](/learn/runtime-layout)：命中测试与无障碍投影所消费的 `LayoutResult` 几何来源
- [窗口与平台](/learn/window-platform)：原生窗口事件与 DPI 缩放如何到达 `handle_window_event`
- [输入验收矩阵（参考）](/reference/input-conformance) · [单次指针事件的成本（参考）](/reference/input-cost)
