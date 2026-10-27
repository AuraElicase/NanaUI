# 工作区、停靠与区域布局

桌面生产力软件通常由侧栏、主工作区、检查器、底部面板以及可拆分或浮出的停靠窗格组成。NanaUI 在 Runtime 中提供这些桌面壳层的结构与交互合同，而每个区域或窗格里展示什么业务内容、如何路由和持久化业务状态，始终由你的应用决定。

如果应用只需要简单的单页或双栏结构，直接使用 `List` 或 `AppShell` 即可，不必引入 `Workspace`。

## Workspace：语义化工作区区域

`WorkspaceLayout` 按注册顺序维护一组 `RegionState`。每个区域（Region）由以下字段定义：

| 字段 | 作用 |
| --- | --- |
| `id` | 内置区域名（如 `Primary`、`Resources`、`Inspector`）或 `Custom(String)` 稳定身份 |
| `role` | 区域角色：导航、资源、主区、检查器、时间线、控制台、工具 |
| `placement` | 停靠方位：`start` / `primary` / `end` / `top` / `bottom` |
| `scope` | 作用范围：`workspace`（整窗级）或 `primary`（主区域内部） |
| `size` / `min` / `max` / `default` | 逻辑像素尺寸及缩放上下限 |
| `fill_priority` | 存在多个 `primary` 区域时如何分配剩余空间 |
| `collapsible` / `resizable` | 是否允许折叠、是否允许拖拽边缘调整尺寸 |
| `narrow_behavior` | 窗口收窄时的响应策略：`shrink` / `collapse` / `overlay` / `none` |

内置 `RegionId` 提供标准六区域预设，应用也可通过 `Custom(String)` 注册任意数量的起止区域和上下区域（`register` 拒绝重复 ID）。

- **单一状态权威**：区域尺寸、折叠状态、可见性以及 JSON 序列化恢复全部由 `WorkspaceModel` 与 `WorkspaceMutation` 持有；宿主侧的 `WorkspaceController` 只负责把指针事件和时钟换算成 `WorkspaceMutation`，不在适配器里存第二份状态。
- **内容挂载与分隔条**：`Workspace` 控件通过 `WorkspaceRegionSlot { id, content }` 将应用的子节点挂入对应区域。分隔条的 `8 px` 命中区居中跨在相邻区域边缘上，自身不占据网格轨道（grid track）。
- **折叠与视口映射**：折叠或隐藏的区域不绘制、不响应拖拽，并在布局几何中让出空间（带 `260 ms` 过渡动画，仅在过渡进行中请求帧）；处于 `overlay` 模式的区域浮在主区之上，不挤占 `primary` 空间。`WorkspaceGeometry` 将布局结果映射为逻辑矩形与物理像素矩形供 GPU 视口消费，自身不创建窗口或 GPU 资源。

## 侧栏、设置页与非模态面板

- **侧栏（`SidebarFrame` / `SidebarSection` / `SidebarRow`）**：提供通用的分组与导航行结构，具体路由跳转与选中态由应用管理。
- **设置页（`SettingsModel` / `SettingsState`）**：维护稳定的设置页签与恢复规则。主题列表由应用的 `ThemeRegistry` 提供并持久化 `ThemeId`，其余业务设置项由应用状态持有。当设置页使用独立的 `WorkspaceController` 时，进入设置页不会覆盖主工作区的区域尺寸与折叠状态。
- **全窗画面上的非模态面板（`OverlayHost` + `Panel`）**：在铺满窗口的实时画面之上需要一块可交互的任务面板时，使用 `OverlayHost` 挂载 `Panel`。它不改变主工作区尺寸，也不锁定面板外的指针或 Tab 焦点遍历；应用只需提供业务内容与视口内缩（`PanelInsets`），面板表面、关闭生命周期与焦点归还由框架处理。

## Dock：页签停靠与浮动窗口

产品级停靠由 Runtime 的 `Dock` 组件与 `DockWorkspace` 模型提供（`nana_ui::dock::*` 中的 `DockController` / `DockAction` 仅是把指针和帧事件翻译成 mutation 的宿主适配器，不是第二套 Dock）：

- 窗格（pane）内容是树上的普通子节点，分割比例（split ratio）、拖拽命中条与落点预览由框架统一计算。
- 当用户把窗格拖出主窗口变为浮动面板时，`DockWorkspace` 发出 `DockWorkspaceEvent`，宿主通过 `runtime_dock_window_update` 将其转换为 `WindowCommand::Open` 或 `Close`（完整接法见 `examples/runtime-host-fixture`）。

## AreaLayout 与专业工具检查器表面

除 `Workspace`（固定语义区域）与 `Dock`（多页签停靠与浮动窗）外，三维、动画与音视频等 DCC 工具常需要「屏幕任意切分」与「高密度属性检查器」：

- **[AreaLayout](../components/area-layout.md)**：将一块矩形递归切分为永不重叠的区域，每个区域展示应用指定的一种编辑器。用户可从区域头部左上/右上角的小方块向内拖拽切分新区域，或向相邻兄弟区域拖拽合并；支持单区域最大化与整套工作区预设（`set_area_layout`）切换。切换预设时，新旧布局共有的区域原地保留子节点（滚动位置与输入状态不丢失），未出现的区域隐藏等待而不销毁。
- **[PropertyGrid](../components/property-grid.md)** 与 **[ScrubField](../components/scrub-field.md)**：检查器式的两列密集属性网格（支持可折叠分组、全网格同步拖拽的公共标签列宽、多选混合值 `mixed` 与附带禁用原因的行遮罩）以及可左右拖动改值、单击键入的数字属性字段。
- **[TimelineEditor](../components/timeline-editor.md)**：单节点自绘的可编辑时间底座，内置帧/秒自适应标尺与播放头、左侧可折叠轨道树、关键点框选与整帧吸附拖动、片段的选择移动修剪（吸附到片段边缘与播放头、不重叠），以及三次贝塞尔曲线视图与切线手柄。

### Workspace、Dock 与 AreaLayout 如何选择

| 壳层组件 | 适用场景 | 核心特征 |
| --- | --- | --- |
| `Workspace` | 标准桌面应用主框架 | 语义化区域（导航、主区、检查器、底栏），支持窄窗自动折叠与 `260 ms` 展开过渡 |
| `Dock` | IDE 式多面板停靠 | 面板可叠放成 Tab 组、拖到边缘拆分，或拖出成为独立浮动窗口 |
| `AreaLayout` | Blender 等 DCC 式工作台 | 区域永不重叠，从角块拖入切分或合并，支持区域头部自定义切换器与预设秒切 |

## 页签重排、列表拖放与命令面板

- **页签与列表重排**：`Tabs::on_reorder` 与纵向的 `ReorderList` 均采用「被移动的项 + 目标位置紧随其后的项（`None` 表示末尾）」描述重排意图，组件自身不篡改业务数组顺序。跨窗口拖拽 Tab 使用 `TabDragGroup` 与 `TabDragSurface`（接收方可通过 `accepts_external_drop(false)` 仅允许拖出而拒绝外部落入）。
- **树视图拖放（`TreeView`）**：将应用提供的稳定节点 ID、展开状态与层级渲染为可折叠树，支持多选、行尾开关列、行内重命名（双击或 `F2`）与拖放排序；拖放回传目标行及 `Before` / `Inside` / `After` 意图，不读取或监听本地文件系统。
- **命令面板（`CommandPalette`）与 `ActionRegistry`**：`ActionRegistry` 登记稳定的 Action ID 与快捷键上下文，`CommandPalette` 仅展示当前上下文中可用的动作；具体的命令分发与快捷键存盘由应用执行。
- **分栏投影（`PaneTree` / `PaneChrome`）**：提供轻量分栏与页签栏渲染投影，窗格 ID、脏标记与跨窗口移动逻辑归应用所有。
