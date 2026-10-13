---
title: 工作台:Shell、Workspace、Dock 与 Overlay
---

# 工作台:Shell、Workspace、Dock 与 Overlay

工作台层(Shell / Workspace / Dock / regions / overlays)是 Runtime 里"桌面应用骨架"的那一部分:它不新增渲染机制,而是把保留树、布局与输入链路组装成标题栏、多区域工作区、可拆分停靠区与模态/非模态浮层。本章的核心线索是**逻辑状态与呈现分离**:区域尺寸、折叠过渡、dock 树、设置都活在可序列化的模型里,组件投影只是把这些状态写成样式与结构变异。读完你能回答:WorkspaceModel 为什么是区域状态的唯一权威、DockWorkspace 与 Dock 组件是什么关系、overlay 激活如何在一个 Runtime 事务里完成、以及这些状态如何持久化。

## 职责与边界

- `nana-ui-core` 持有后端中立的**逻辑模型**:`WorkspaceModel` / `WorkspaceLayout` / `RegionState`(`workspace_model.rs`、`layout.rs`)、`AppearanceSettings` / `SettingsModel`(`settings.rs`)、持久化能力 `ViewStateStore`(`persist.rs`)。它们不依赖窗口或渲染后端(`crates/nana-ui-core/src/workspace_model.rs:83` 的类型文档:"Owns persisted region layout, resize interaction, viewport facts and collapse transitions without depending on a window or renderer backend")。
- `nana-ui-runtime` 持有**组件与编排**:`Workspace` / `DesktopShell` / `AppShell` / `AppTitleBar`(workspace.rs、shell.rs)、`DockNode` / `DockWorkspace` / `Dock`(dock.rs)、overlay 激活与路由(framework/overlay.rs、framework/modal.rs)、侧栏/面板/分栏/通知等呈现组件(sidebar.rs、panel.rs、pane.rs、pane_section.rs、toast_stack.rs、overlay_surfaces.rs)。
- 宿主层(`nana-ui`)只做**适配**:`WorkspaceController` 把 Instant 时钟与指针事件翻译成 `WorkspaceMutation`,"This type does not store a second copy of that state"(`crates/nana-ui/src/workspace.rs:74`)。
- 业务内容留在消费方:AGENTS.md 规定框架只保留骨架合同,应用业务状态、导航与 region 内容属于使用 NanaUI 的产品。
- 序列化边界:只有逻辑模型进存档;组件投影、overlay 激活态与 dock 的内容绑定不进存档("Live slot contents and [`DockWorkspace::primary`] are not persisted",`crates/nana-ui-runtime/src/dock.rs:555`)。

## 依赖与邻居

| 层 | 提供什么 |
| --- | --- |
| `nana-ui-core` | 区域模型、设置模型、`ViewStateStore`、动效计时(`MotionTiming`) |
| `nana-ui-runtime` | 组件投影、resize/drag 交互、overlay 路由、dock 树操作 |
| 输入链路(见[输入链路](/learn/runtime-input)) | 指针按下/移动经 dispatch 链进入 `begin_workspace_resize` 等入口 |
| 布局引擎(见[布局引擎](/learn/runtime-layout)) | 区域轨道、dock 分割条、overlay scrim 的盒子几何 |
| `nana-ui` 宿主 | 窗口、标题栏原生控件、浮动 dock 窗口、设置存盘时机 |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `WorkspaceMutation` | `crates/nana-ui-core/src/workspace_model.rs:13` | 区域交互的 12 种逻辑变异(Toggle/Resize/SetViewport…) |
| `WorkspaceModel` | `crates/nana-ui-core/src/workspace_model.rs:85` | 区域布局 + 折叠过渡 + 拖拽 + 视口事实,单一权威 |
| `RegionState` / `RegionId` | `crates/nana-ui-core/src/layout.rs:94` / `:15` | 一个区域的结构与持久化合同(placement/scope/尺寸约束) |
| `WorkspaceLayout` | `crates/nana-ui-core/src/layout.rs:386` | `Vec<RegionState>` + 默认六区域 |
| `Workspace` | `crates/nana-ui-runtime/src/workspace.rs:114` | 树上的工作区组件,持有模型快照与 chrome 槽位 |
| `DockNode` / `DockWorkspace` | `crates/nana-ui-runtime/src/dock.rs:59` / `:296` | 递归 dock 树 / 产品级 dock 权威 |
| `DockDropZone` | `crates/nana-ui-runtime/src/dock.rs:49` | 拖放目标:Left/Right/Top/Bottom/Tab |
| `RuntimeOverlayKind` | `crates/nana-ui-runtime/src/framework/overlay.rs:14` | Dialog/Panel/Menu/Tooltip/Status 五种浮层行为 |
| `OverlayHost` | `crates/nana-ui-runtime/src/view_components.rs:3478` | overlay 挂载点组件(节点 tag `overlay-host`) |
| `OverlayVisibility` | `crates/nana-ui-runtime/src/overlay_visibility.rs:55` | 媒体 chrome 自动隐藏状态机 |
| `PaneTree` / `PaneSection` | `crates/nana-ui-runtime/src/pane.rs:616` / `crates/nana-ui-runtime/src/pane_section.rs:41` | 分栏树 / 区段(标题+标签+主体) |
| `ViewStateStore` | `crates/nana-ui-core/src/persist.rs:146` | 带版本信封的视图状态持久化能力 |
| `AppearanceSettings` | `crates/nana-ui-core/src/settings.rs:101` | 圆角/窗口材质/毛玻璃目标等外观设置 |
| `SettingsModel` / `SettingsState` | `crates/nana-ui-core/src/settings.rs:572` / `:651` | 设置页模型与选中标签页状态 |

## 内部结构

### 1. 逻辑状态与呈现分离

`Workspace` 组件的类型文档(`crates/nana-ui-runtime/src/workspace.rs:108`)写着规则:"Snapshot the model; `project` never owns a clock"——区域可见性、overlay 态、extent、resize 高亮都直接查 `Self::model`,**没有镜像缓存**,`WorkspaceModel` 是这些派生态的唯一权威。所有交互都走 `Workspace::apply(mutation, now)`(`crates/nana-ui-runtime/src/workspace.rs:156`),而 `now` 由调用方传入,模型自身不持时钟。组件自身的字段也只有装配产物与模型快照:`slots`/`style`/`layout`/`workspace_corners`/`handles`/`middle`/`primary_column`/`primary_row`/`editor_stack`/`model`(`workspace.rs:114` 起),没有第二个状态源。

当 `Workspace` 挂在 `DesktopShell` 下时,任何成功变异后 `write_back_shell_model`(`crates/nana-ui-runtime/src/workspace.rs:1185`)把组件里的模型克隆回父 shell 的 `model` 字段——同一份逻辑状态在两处可见,但写入方向单一。`DesktopShell`(`crates/nana-ui-runtime/src/shell.rs:884`)是"页面级桌面组装器":接宿主挂载的 `AppTitleBar`、`Workspace`、`AppShell` 布局与 `OverlayHost`,应用内容留在 region 槽位里(primary/navigation/inspector/bottom/`extra_regions`,`shell.rs:885-894`)。

### 2. regions:区域模型与默认工作区

区域由 `RegionId`(`GlobalNavigation` / `SectionNavigation` / `Resources` / `PrimaryToolbar` / `Primary` / `Inspector` / `Diagnostics` / `Custom(String)`,`crates/nana-ui-core/src/layout.rs:15`)标识,`RegionState`(`:94`)携带 `placement`(Start/Primary/End/Top/Bottom)、`scope`(Workspace/Primary)、尺寸约束(`size`/`min_size`/`max_size`/`fill_priority`)、`collapsible`/`resizable`、以及响应式收窄行为 `NarrowBehavior`(Shrink/Collapse/Overlay/None,`:85`)。`WorkspaceLayout` 的默认值(`crates/nana-ui-core/src/layout.rs:390`)给出标准六区域:

| 区域 | 默认尺寸 | min / max | 说明 |
| --- | --- | --- | --- |
| `GlobalNavigation` | 56px | 44 / 96 | 全局导航条 |
| `Resources` | 260px | 180 / 520 | 资源栏,可折叠可缩放 |
| `PrimaryToolbar` | 34px | — | Primary scope 顶部工具栏 |
| `Primary` | 填充 | min 320 | `fill_priority = 1` 的主内容区 |
| `Inspector` | 280px | 200 / 560 | 检查器,可折叠可缩放 |
| `Diagnostics` | 200px | 96 / 520 | Primary scope 底部诊断台 |

折叠过渡是模型内的采样动画:`RegionTransition` 用 `MotionTiming` + `EaseInOutCubic` 在 `from_extent` 与 `to_extent` 之间插值(`crates/nana-ui-core/src/workspace_model.rs:44`),时长 `WORKSPACE_REGION_TRANSITION_DURATION`(`:10`)。两个细节值得注意:已到结束时刻但尚未被 `AdvanceAnimations` 回收的过渡**不参与**可见性/宽度判定(`active_transition` 的注释,`:157-159`);几何采样时 `with_transient_extents`(`crates/nana-ui-core/src/layout.rs:450`)把过渡中的 extent 临时写进布局快照,渲染侧看到的是连续动画,而持久化布局从未被中间值污染。

### 3. Workspace 组件:投影、chrome 与 resize 链

`assemble_workspace`(`crates/nana-ui-runtime/src/workspace.rs:901`)按需创建轨道 chrome(middle track、primary stack/track、editor stack)与每个可缩放区域的 resize 手柄,全部是 detached 组件,创建结果写回组件状态。投影阶段(`project_root` / `project_track_host` / `project_region`,`workspace.rs:263-355`)用 `MutationQueue` 写样式:区域容器带 resize 手柄时 `overflow` 放开为 `Visible`——"8px bar is centered on the painted edge, so half sits in the neighboring track and must not be clipped by this region"(`workspace.rs:349-350`)。

指针 resize 走三段式,入口由输入链路的 dispatch 调用:`begin_workspace_resize`(`crates/nana-ui-runtime/src/workspace.rs:1041`)把 `ResizeStart` + `ResizeMove` 应用到模型,成功则在**同一事务**里 `request_focus` + `capture_pointer`(`:1063-1064`),再 `write_back_shell_model`;`update_workspace_resize`(`:1074`)与 `end_workspace_resize`(`:1100`)凭指针捕获找到手柄,继续应用 `ResizeMove` / `ResizeEnd`。输入链路通过 `WORKSPACE_HOOKS` 静态表(`workspace.rs:1230`)解耦地到达这些入口——路由器只在 workspace 存在时才碰这些手柄。宿主侧,`WorkspaceController`(`crates/nana-ui/src/workspace.rs:78`)把 `WorkspaceAction`(Instant 时钟的窗口/指针事件,`:56`)翻译成 `WorkspaceMutation`,自己不存第二份状态。

### 4. Dock:递归树、权威与分割公式

`DockNode`(`crates/nana-ui-runtime/src/dock.rs:59`)是三态递归树:`Item`(id + 可选内容节点)、`Tabs`(标签序列 + active + 内容绑定)、`Split`(axis + ratio + 两棵子树),`ratio` 是 clamp 后的首子份额。产品级权威是 `DockWorkspace`(`:296`):主树 + 浮动表面集合 + hidden 集合 + primary id;类型文档(`:286-294`)明确:"`Dock` is one in-tree surface projection of this workspace; `nana_ui::dock::DockController` is a host adapter, not a second live dock",宿主应为每个 `DockSurfaceSpec`(`surfaces()`,`:329`)在对应窗口文档上建一个 `Entity<Dock>` 再 `AppContext::assemble_dock`(`:2430`)。

浮动是事件而非直接副作用:`float_item_at`(`:396`)检查 `locked`/`item_limits`/可隐藏性,从主树摘出该 item,返回 `DockWorkspaceEvent::OpenFloating` 让宿主去开窗口;宿主再把窗口移动/关闭回报给 `apply`(`:436`,Close/Move/FocusFloating)。窗口身份由 `dock_surface_window_key`(`:251`)确定性地从表面 id 派生(FNV-1a,十进制串映射自身,主表面即主窗口)。

树上的 `Dock` 组件(`:1301`)只是一个表面的投影;组装入口是 `assemble_dock_panels`(`:2413`)与 `AppContext::assemble_dock`(`:2430`)。本章所有指针交互都是同一形状的三段式,入口由[输入链路](/learn/runtime-input)的 dispatch 链驱动:

| 交互 | begin / update / end |
| --- | --- |
| 区域 resize | `begin_workspace_resize` `workspace.rs:1041` / `update_workspace_resize :1074` / `end_workspace_resize :1100` |
| dock 分割条拖拽 | `begin_dock_split_resize` `dock.rs:2602` / `update :2647` / `end :2701` |
| dock 项拖放 | `dock.rs:2771` / `:2808` / `:2851`(落点为 `DockDropZone`) |

分割份额有一组共享常量(`dock.rs:27-33`):`DOCK_DIVIDER_HIT_SIZE = space::MD`(分割条命中厚度,"Host adapters must not invent a second divider size")、`MIN_SPLIT_RATIO = 0.05`、`MAX_SPLIT_RATIO = 0.95`、`DOCK_SPLIT_KEYBOARD_STEP = MIN_SPLIT_RATIO`。指针拖拽的**唯一**份额公式是 `dock_split_ratio_from_pointer`(`:796`,文档:"This is the only live split-ratio formula; `nana_ui::dock::DockController` must call it");键盘微调用 `dock_nudge_split_ratio`(`:806`),由 `adjust_focused_dock_split`(`:2733`)在聚焦分割条上驱动。

分割条拖拽与 dock 项拖拽和 workspace resize 一样是三段式:`begin_dock_split_resize`(`dock.rs:2602`)、`update`(`:2647`)、`end`(`:2701`)由输入链路驱动;dock 项跨表面拖放(`begin :2771` / `update :2808` / `end :2851`)以 `DockDropZone`(Left/Right/Top/Bottom/Tab,`:49`)表达落点——分割还是成组为标签,由落区决定,而不是两条交互路径。tab 顺序调整是纯树操作 `reorder_tab`(`:171`)。

dock 内容侧的分栏由 `PaneTree`(`crates/nana-ui-runtime/src/pane.rs:616`)承担,`PaneTreeNode`(`:381`)是 leaf/split 二态。它给每个宿主内容一个 `PaneSlot`(`:485`),设计理由写在文档里:早期实现把宽度/弹性直接写到宿主内容节点上,而宿主自己的投影会覆盖它们——"Owning a box per split and per leaf is what gives the tree somewhere to put a number that stays put"(`pane.rs:476-483`)。份额用**长度**而非 `flex_grow` 权重表达,"因为 `LayoutStyle::child_main_length` 对一切会增长的东西都回答 Fill"(`pane.rs:536-539`)——这正是布局引擎(见[布局引擎](/learn/runtime-layout))尺寸合同的反向引用。区段 `PaneSection`(`crates/nana-ui-runtime/src/pane_section.rs:41`)是"标题 + 标签行 + 弹性主体"的复合卡:标签行不铺卡面,自己补内容线内边距,"that is what keeps tab labels and selection pills on the card content line"(`:33-38`);其槽位 `PaneSectionSlot`(`:225`)承诺"The section writes pane geometry onto it; nothing else touches style"——区段是它唯一的样式书写者。

### 5. overlays:分层、激活与自动隐藏

`AppShell`(`crates/nana-ui-runtime/src/shell.rs:704`)是"标题栏 + 填充 body + overlay 兄弟层"的三槽结构;overlay 槽被投影为四边归零的 absolute 层,z-index 默认 `OVERLAY_Z_INDEX = 1`(`shell.rs:29`),状态浮层更高一档 `STATUS_OVERLAY_Z_INDEX = 2`(`:30`)。标题栏是宿主挂载的 `AppTitleBar`(`shell.rs:82`,36px,leading/center/trailing/controls 四段),其窗口按钮发出的是类型化命令 `WindowChromeAction`(Minimize/ToggleMaximize/Close,`:38`)——"Typed window command. Runtime never touches a platform window handle";外观背景可选 `TitleBarBackdrop`(Themed/Transparent/OverMedia,`:69`)。指针事件的归属权写在注释里(`shell.rs:788-794`):如果槽位是 `OverlayHost`,是否接收指针由**激活中的 overlay 种类**决定,而激活可以在 shell 不重投影时变化,所以这个字段必须由 host 拥有;普通 overlay 槽则"持有内容(一个未隐藏的子节点)时接收指针"。

`OverlayHost` 组件(`crates/nana-ui-runtime/src/view_components.rs:3478`)投影时确保世界里有 `OverlayHostState`,并保持指针透明起步——"Whether a host takes the pointer follows the active overlay's kind, which activation resolves and writes"(`:3514-3517`)。种类由角色决定:`overlay_kind_for_role`(`crates/nana-ui-runtime/src/framework/overlay.rs:44`)把 `AccessibilityRole::Dialog/Region/Menu/Tooltip/Status` 映射到 `RuntimeOverlayKind`,`Dialog|Menu` 阻断指针并困住焦点,`Tooltip|Status` 是被动的("showing or hiding one must leave focus where the user put it",`:31-35`)。激活走 `AppContext::activate_overlay`(`crates/nana-ui-runtime/src/framework/modal.rs:221`):"Activate one direct overlay child and move focus into its retained subtree in the same Runtime transaction"——焦点迁移、`OverlayHostState` 写入、事件发射在一个事务内,不会出现"浮层已显示但焦点还在底下输入框"的中间帧。

模态表面(`ConfirmDialog`、`Drawer`,overlay_surfaces.rs)与命名面板(`Panel`,panel.rs:"A named nonmodal region… The surface alone receives pointer input; the stage around it and the document's ordinary Tab order remain available",`crates/nana-ui-runtime/src/panel.rs:55-61`)都挂 OverlayHost,经 `activate_overlay` / `dismiss_overlay`(`modal.rs:352`)开关;激活/关闭都会发 `OverlayChanged`(`modal.rs:200-215`),宿主可据此同步窗口级状态。模态表面的内部几何由 `ModalSurfaceSections`(`crates/nana-ui-runtime/src/overlay_surfaces.rs:106`)以卡片坐标组织,供表面绘制器消费;`ModalInitialFocus`(`:21`)与 `ModalBehavior`(`:29`)声明初始焦点与关闭行为。命名面板还接受 `PanelInsets`(`panel.rs:19`,"Logical pixels reserved by the application for window chrome and tools"),让应用声明为原生 chrome 与工具预留的逻辑像素。

媒体 chrome 的自动隐藏由 `OverlayVisibility`(`crates/nana-ui-runtime/src/overlay_visibility.rs:55`)驱动:指针/菜单/拖拽锁(`OverlayLocks`,`:14`)阻止倒计时,输入路由把事件汇成 `OverlayActivity` 喂给 `route_auto_overlays`(`crates/nana-ui-runtime/src/framework/auto_overlay.rs:125`)。状态机对外的信号是 `OverlayVisibilityChanged`(`overlay_visibility.rs:49`)——"Emitted by the bar whichever way its policy is driven, so chrome outside the bar (a title bar over the same picture) can show and hide with it";驱动入口还有 `reveal_overlay`(`:334`)/`conceal_overlay`(`:349`)/`step_overlay`(`:295`)/`overlay_wakeup`(`:361`)。

通知用 `ToastStack`(`crates/nana-ui-runtime/src/toast_stack.rs:43`):新 toast 先布局后显示,指针悬停或焦点在内时全部计时器暂停,"The stack itself takes no pointer input"(`:36`)——间隙与上方空间把事件透给下层。API 是 `push_toast`(`:321`,组装并先布局)、`remove_toast`(`:364`)、`clear_toasts`(`:400`)、`shown_toasts`(`:421`);`max_visible`(`:68`)之外的新 toast 等有可见名额再上屏,`ToastDismissed`/`ToastAction` 之后是否移除由应用决定——框架只提供呈现,不代答业务。侧栏家族(`SidebarFrame` `sidebar.rs:182`、`SidebarSection` `:695`、`SidebarRow` `:360`、`SidebarRowIcon` `:625`、`SidebarFooter` `:1300`)则是同一模式在导航区的展开:模型在外,组件只投影行与分组 chrome。

### 6. 序列化:版本信封与历史兼容

三套持久化共用 `ViewStateStore`(`crates/nana-ui-core/src/persist.rs:146`):"Canonical view-state capability, independently scoped from settings and app KV"——按 `RestorationPath` 作用域寻址,载荷装在带 `schema_version` 的信封里;版本不匹配或损坏的条目**直接删除**并以 `SCHEMA_MISMATCHES` / `CORRUPTIONS` 计数(`:174-185`),命中/未命中分别计量。底层 `LocalStorageAdapter`(`:109`)是前端能力,"no access to raw physical keys or backend flush"。典型调用:

- 区域布局:`WorkspaceModel::layout_json` / `restore_layout_json`(`crates/nana-ui-core/src/workspace_model.rs:187` / `:191`),恢复会同时清掉进行中的 resize、悬停与过渡。
- dock:`DockWorkspace::layout_json` / `restore_layout_json` / `save_to_store` / `restore_from_store`(`crates/nana-ui-runtime/src/dock.rs:499-529`)。投影类型 `DockWorkspacePersist`(`:559`)的字段名刻意匹配历史宿主适配器 JSON(`version`、tagged `kind` 节点、数字 `surface`、`bounds`、`monitor`、`hidden`、`locked`);"Live slot contents and [`DockWorkspace::primary`] are not persisted"(`:555-556`)——内容绑定是运行时事实,不进存档。
- 设置:`AppearanceSettings`(`crates/nana-ui-core/src/settings.rs:101`,圆角四档、`WindowMaterialMode` `:51`、`BackdropTarget` `:85`、`titlebar_follows_sidebar`)有 `to_json`/`restore_json`/`save_to_store`(`:279-289`),`to_json_with_theme_id`(`:311`)把当前主题 id 一并写入;`SettingsState`(选中标签页)单独 `to_json`(`:680`)。设置数据旁定义了 `AppearanceEvent`(`:700`),注释说明动机:"Kept next to settings data so Vue and Runtime can share the same contract"。

## 协作与数据流

一次"拖动检查器边缘"的完整链路:指针按下 → 输入 dispatch 链的 workspace 分支 → `begin_workspace_resize` → `WorkspaceMutation::ResizeStart/ResizeMove` 应用到 `WorkspaceModel`(尺寸 clamp、过渡状态)→ `write_back_shell_model` 同步父 shell → 组件重投影把新 extent 写进区域样式变异 → `UiWorld::commit` 进入保留树(见[运行时保留树](/learn/runtime-tree))→ 布局引擎重排相邻轨道 → 帧边界 `RuntimeDocument::flush` 排水 → 场景提取。dock 分割条拖拽、tab 重排(`reorder_tab`,`dock.rs:171`)、浮层激活走的是同一形状:交互入口 → 模型变异 → 组件投影 → 变异队列 → 布局。

把一个 dock 项拖出成浮动窗口,是"事件而非副作用"的完整样本:`float_item_at`(`dock.rs:396`)在 `locked`/`item_limits`/可隐藏性检查通过后,把 item 从主树摘出并返回 `DockWorkspaceEvent::OpenFloating`;宿主开窗(窗口身份由 `dock_surface_window_key`(`:251`)从表面 id 确定性派生),在对应窗口文档上按 `DockSurfaceSpec` 组装新的 `Dock` 表面;窗口被用户关闭时,宿主以 `apply(CloseFloating)`(`:436`)回报,模型回到一致状态。整条链上没有任何一步直接操作平台窗口——Runtime 只发出与回收事件。

宿主在窗口关闭、设置变更等时机调用各模型的 `save_to_store`;重启时按版本信封恢复,坏档自愈为默认。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-core/src/workspace_model.rs` | `WorkspaceMutation`/`WorkspaceModel`:区域交互、过渡采样、JSON |
| `crates/nana-ui-core/src/layout.rs` | `RegionId`/`RegionState`/`WorkspaceLayout`/`NarrowBehavior` |
| `crates/nana-ui-core/src/settings.rs` | `AppearanceSettings`/`SettingsModel`/`SettingsState` 及存取 |
| `crates/nana-ui-core/src/persist.rs` | `ViewStateStore` 版本信封、`LocalStorageAdapter`、`AppSettings` |
| `crates/nana-ui-core/src/geometry.rs` | `WorkspaceGeometry` 等几何类型(`:241`) |
| `crates/nana-ui-runtime/src/shell.rs` | `AppTitleBar`/`AppShell`/`DesktopShell`、窗口命令按钮 |
| `crates/nana-ui-runtime/src/workspace.rs` | `Workspace` 组件、chrome 组装、resize 三段式、hooks |
| `crates/nana-ui-runtime/src/dock.rs` | `DockNode`/`DockWorkspace`/`Dock` 组件、拖拽与键盘调整、持久化 |
| `crates/nana-ui-runtime/src/framework/overlay.rs` | `RuntimeOverlayKind`、overlay 指针/按键路由 |
| `crates/nana-ui-runtime/src/framework/modal.rs` | `activate_overlay`/`dismiss_overlay` 事务 |
| `crates/nana-ui-runtime/src/framework/auto_overlay.rs` | 输入事件 → 自动隐藏 chrome 的 `OverlayActivity` |
| `crates/nana-ui-runtime/src/overlay_surfaces.rs` | `ConfirmDialog`/`Drawer`/`ModalChrome` |
| `crates/nana-ui-runtime/src/overlay_visibility.rs` | 媒体 chrome 自动隐藏状态机 |
| `crates/nana-ui-runtime/src/panel.rs` / `pane.rs` / `pane_section.rs` | 命名面板 / 分栏树与标题行 / 区段槽位 |
| `crates/nana-ui-runtime/src/sidebar.rs` / `toast_stack.rs` | 侧栏框架与行 / 通知栈 |
| `crates/nana-ui/src/workspace.rs` | `WorkspaceController` 宿主适配(Instant→Duration) |

## 延伸阅读

- [运行时保留树](/learn/runtime-tree):组件投影产生的变异如何提交、系统如何排水。
- [输入链路](/learn/runtime-input):resize 手柄、dock 分割条、overlay 指针如何被路由到本章的交互入口。
- [布局引擎](/learn/runtime-layout):区域轨道与 dock 分割的盒子几何如何算出。
- [模块与分层](/learn/modules-and-layers):core 与 runtime 的总体分层。
- [合同与不变量](/learn/contracts-and-invariants):逻辑状态与呈现分离在跨层合同中的位置。
