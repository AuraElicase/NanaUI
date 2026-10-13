---
title: 输入链路
---

# 输入链路

输入从原生窗口(或浏览器、无头会话)到达一个保留树节点,中间要经过两层合同:`nana-ui-input` 定义"事件长什么样",`nana-ui-runtime` 的 `framework/input.rs` 决定"事件落到谁头上"。本章沿这条链路走一遍:规范化事件与序列号、端点与合并、`AppContext::route_input` 的验证与指针本地化、指针/滚轮/按键/输入法四条分发链、焦点与 roving focus、IME 与 `HostServices` 回写,以及无障碍投影。读完你能回答:为什么宿主只被要求"降维一次"、为什么文本会被路由器丢弃、为什么滚动后无障碍缓存也能对上。

## 职责与边界

- `nana-ui-input`:唯一的规范化输入合同。规范事件的形状、身份、顺序、端点与宿主服务回调,不含任何平台代码。`crates/nana-ui-input/src/lib.rs:5` 明确它没有可选依赖与平台依赖,`nana-ui-platform` 只是把整套合同原样再导出。
- `nana-ui-runtime` 的 `framework/input.rs`:每 `AppContext` 一个的输入路由器。`crates/nana-ui-runtime/src/framework/input.rs:1` 的模块文档称它是"a host's input events 进入保留世界的唯一路径";所有路由状态都挂在源所绑定的 context 上,因此两个窗口永远不共享指针、IME owner 或光标。
- 宿主适配层(不属本章 crate):原生窗口在 `crates/nana-ui/src/scene_host/input.rs` 把 winit 事件压入端点;Vue/JS 宿主在 `crates/nana-ui-vue/src/host/input_projection.rs:386` 把 JS 采样 `to_canonical()` 后走同一条 `route_input`。

## 依赖与邻居

| 层 | 角色 |
| --- | --- |
| `nana-ui-input` | 合同:事件、序列、端点、`HostServices` |
| `nana-ui-platform` | 仅再导出输入合同,供窗口层使用 |
| `nana-ui-runtime` | 路由、分发、焦点、IME、无障碍投影、命中测试 |
| `nana-ui` / `nana-ui-vue` | 宿主:把原生输入降维成规范事件,把 `HostServices` 落到窗口/JS |
| `nana-text` | 路由文本时需要 shaper 参数(`route_input` 的 `text_shaper`) |
| `nana-diagnostics` | 端点与路由计量(`INPUT_*` 系列,高频数据走 metric 而非逐帧事件) |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `CanonicalInputEvent` | `crates/nana-ui-input/src/canonical.rs:44` | 元数据 + 载荷,所有宿主事件的统一外壳 |
| `InputPayload` | `crates/nana-ui-input/src/canonical.rs:209` | 13 种载荷:指针/滚轮/按键/文本/输入法/文件拖放/焦点/连接断开 |
| `CommittedText` | `crates/nana-ui-input/src/canonical.rs:269` | 已提交文本,携带产生它的按键序列号 |
| `CompositionInput` | `crates/nana-ui-input/src/canonical.rs:191` | IME 生命周期载荷(Start/Update/Commit/End/DeleteSurrounding) |
| `InputSequencer` | `crates/nana-ui-input/src/canonical.rs:333` | 为事件盖序列号/时间戳,代际递增 |
| `InputEndpoint` | `crates/nana-ui-input/src/canonical.rs:410` | 有界单源收件箱,可合并移动/滚轮 |
| `HostServices` | `crates/nana-ui-input/src/host_services.rs:97` | 光标/IME/剪贴板回调合同 |
| `InputRouteOutcome` | `crates/nana-ui-runtime/src/framework/input.rs:36` | 单事件路由结果 |
| `InputRouteError` | `crates/nana-ui-runtime/src/framework/input.rs:74` | 路由拒绝分类(见下表) |
| `KeyStroke` | `crates/nana-ui-runtime/src/framework/input/dispatch.rs:55` | 按键与已提交文本的统一键链载荷 |
| `SourceEffects` | `crates/nana-ui-runtime/src/framework/input/effects.rs:43` | 光标/text-input 最新值槽(宿主回写节流) |
| `InputState` / `SourceState` | `crates/nana-ui-runtime/src/framework/input.rs:247` / `:167` | 路由器全局与每源状态 |
| `HeadlessInput` | `crates/nana-ui-runtime/src/framework/input/headless.rs:19` | 测试/无头输入源 |

## 内部结构

### 1. 规范事件:身份、序列与时间戳

`canonical.rs` 的模块文档(`crates/nana-ui-input/src/canonical.rs:1`)给定了三个硬约束:宿主在边界处一次性降维原生消息;时间戳是 Runtime 动画时钟域的纳秒(悬停节流、点击计时与宿主动画同一时钟);序列号在一个端点代际内跨设备严格递增。

六个身份类型(`InputSourceId`、`DeviceId`、`PointerId`、`EndpointGeneration`、`InputSequence`、`InputTimestamp`)全是由 `identity!` 宏生成的 `pub u64` 包装(`crates/nana-ui-input/src/canonical.rs:15`)。按键载荷把 `PhysicalKey`(`Cow<'static, str>`,与键盘布局无关)与 `LogicalKey`(永远不是已提交文本)分开;指针载荷统一承载鼠标/触摸/笔,含压感与倾斜;`CompositionInput` 用 UTF-8 字节区间描述选区。

`CommittedText`(`crates/nana-ui-input/src/canonical.rs:269`)是理解文本路由的钥匙:它记录产生这段文本的按键序列号。"一个已被 Runtime 处理的按键(快捷键、焦点遍历、字段提交)不再插入文本:路由器丢弃以它命名的文本,正如浏览器跳过被 preventDefault 的 keydown 之后的 input。"这与 `dispatch_keystroke` 里的 `KeyStroke` 设计呼应——已提交文本被建模为"空键的按下"(`crates/nana-ui-runtime/src/framework/input/dispatch.rs:55`),因此能进入与按键完全相同的编辑链。

排序与缓冲由序列器和端点两层承担。`InputSequencer::stamp`(`crates/nana-ui-input/src/canonical.rs:362`)在 u64 耗尽时 panic 并要求推进端点代际;`advance`(`:383`)递增代际、清零序列,文档强调"代际绝不能重复,否则旧端点的事件与新端点无法区分"。

`InputEndpoint`(`crates/nana-ui-input/src/canonical.rs:410`)是有界单源收件箱。设计取向写在类型文档里:满了就把事件还给调用者,而不是丢掉一个按键、按钮、焦点或 IME 转移;拒绝与否是路由器在 drain 时的决定。`coalesce`(`:470`)只合并相邻的指针移动与滚轮(其余载荷都是转移,不可合并),且要求序列号推进、时间戳不回退——"被合并的样本不能对验证器隐藏一次序列/时间戳回退"。滚轮合并把 delta 求和、保留最后位置。原生窗口的容量是 `ENDPOINT_EVENTS = 1024`、`ENDPOINT_PAYLOAD_BYTES = 1MiB`(`crates/nana-ui/src/scene_host/host_services.rs:22`),字节预算只限制突发:一次多文件拖放本身就能入队,drain 后的重试必然成功。

### 2. 路由:绑定、验证与指针本地化

宿主先 `bind_input_source` 把源绑到文档,再逐事件 `AppContext::route_input`(`crates/nana-ui-runtime/src/framework/input.rs:397`,唯一定义)或批量 `drain_input`(`:430`)。`drain_input` 的注释说明每个事件先出队再路由,"事件自身的副作用不会把它卡在队头"。`route_next`(`:451`)是观察者式宿主的节奏:一次路由一个事件,让程序先观察。

`route_validated`(`:508`)按固定顺序拒绝:`UnknownSource → StaleGeneration → Disconnected → OutOfOrder(序列必须大于上次)→ TimestampRegression(时间戳不能小于上次)`。通过后按载荷分发,指针先被 `localize`(`:64`)改写成 context 本地指针 id——"平台 id 在失焦或同 context 另一窗口之后会映射到新 id",观察者拿到的是路由器用来管理捕获与悬停的那个 id。拒绝分类即 `InputRouteError`(`:74`),绑定期错误是 `InputBindError`(`:112`,`StaleGeneration`/`DocumentRebind`):

| 变体 | 触发条件 |
| --- | --- |
| `UnknownSource` | 源未 `bind_input_source` 或已解绑 |
| `StaleGeneration` | 事件属于旧端点世代(端点重启过) |
| `Disconnected` | 设备已断开且事件晚于断开 |
| `OutOfOrder` | 序列号没有严格大于该源上一事件 |
| `TimestampRegression` | 时间戳早于该源上一事件 |
| `Dispatch` | 分发链内部错误,原样上抛 |

每个事件路由后调用 `flush_reactive()`("处理器写了信号;先应用绑定,再判断这一事件是否使帧失效"),失效与否由 `pending_work_revision` 的增量回答(`InputRouteOutcome::invalidated_work`)。没有输入事件的世界变化由 `refresh_input_effects`(`:478`)兜底:宿主在每帧后调用,它在静止指针下重推命中——"滚轮滚动了、布局移动了、某行动画进来了,悬停跟随其后,像浏览器在帧后所做的那样"。

### 3. 四条分发链

`dispatch.rs` 的模块文档(`crates/nana-ui-runtime/src/framework/input/dispatch.rs:1`)概括为:"指针与滚轮共享一次命中;按键与已提交文本走同一条键链;合成事件进入聚焦的编辑器"。

- **指针**(`dispatch_pointer :88`):先解析落点,"一次命中查询,之后每个读到它的代码都复用它——未捕获的样本一次 hit 查询,捕获的一次也不用"。捕获指针绕过 overlay 路由;滚动条车道在内容之前认领指针("它底下的行失去悬停与 tooltip;被捕获的指针跟随捕获,拖拽亦然")。按下链从弹层外点dismiss(消费该按下,否则释放时无处匹配激活)开始,经滚动条拖拽、可达手柄,到 `focus_node_in_place`("按下已落在目标上,再滚动显示会把内容从释放底下滚走")。

  移动与按下各有一条固定的组件链:Move 依次询问文本区缩放手柄、富文本、图片查看器、文本编辑器拖拽、文档文本选择拖拽、滚动条拖拽、range/span/XY-pad、图表面板/小地图、重排、split 分隔条、dock 分割、workspace 缩放、dock 项、悬停探针;Down 在弹层 dismiss 之后走文本区缩放、滚动条、可达手柄、就地聚焦/清焦、文本编辑器与文档文本的按下、组件按下链(钩子/富文本/图片/图表/重排/dock/split/workspace),最后 `press_pointer` 与步进器/range/XY-pad。链是顺序的:前面的消费者拿了,后面的就看不到;这既是确定性,也是「谁最具体谁先」的优先级声明。
- **滚轮**(`dispatch_wheel :690`):行 delta 乘 `LINE_SCROLL_EXTENT = 60.0`;delta 已定向——"平台把 Shift+滚轮留在垂直方向的宿主,在路由前把它转成水平"。
- **按键**(`dispatch_keystroke :800`):一条有序链——阻断 overlay、应用键策略(只见键不见文本)、聚焦终端、富文本编辑器、纯文本编辑器("聚焦的纯文本编辑器先拥有自己的编辑键:光标移动、选区、删除、缩进、配对"),然后 Tab 顺序焦点、分段控件导航、撤销/重做、剪贴板快捷键(经 `HostServices`)、聚焦控件键、表格导航,最后 `roving_focus_key` 与 `keyboard_menu_key`("两者都在聚焦控件之后运行……组自己的轴向优先于弹出触发器的方向键")。
- **输入法**(`dispatch_composition :1252`):终端、富文本编辑器各自处理;否则若聚焦可编辑字段拥有 IME,`Start → set_ime_preedit("")`、`Update → set_ime_preedit`、`Commit → commit_ime`、`DeleteSurrounding → delete_ime_surrounding`;`Disabled` 时把遗留 preedit 提交掉("迟到的提交绝不能……")。

键链上还有两处值得记住的设计。其一是布局无关的快捷键:`shortcut_key`(`crates/nana-ui-runtime/src/framework/input.rs:214`)在 Ctrl/Cmd 加单个非 ASCII 字母(西里尔、希腊、希伯来)时,按物理键映射回拉丁字母——俄语键盘上的 `KeyQ` 仍回答 `q`,以拉丁写就的快捷键在任何布局下成立。其二是文本抑制的另一半:`SourceState::handled_key`(`framework/input.rs:167`)记录最近被 Runtime 处理的按键按下,`CommittedText.key` 与它相等时文本被丢弃,计入 `text_suppressed`(见下文计数器)。

### 4. 焦点与 roving focus

指针按下用 `focus_node_in_place`(`crates/nana-ui-runtime/src/framework.rs:3059`,等价 web 的 `focus({preventScroll:true})`),程序化聚焦用 `focus_node`(`:3040`,滚动进每个祖先滚动口)。两者最终都收敛到 `MutationQueue::request_focus`(`crates/nana-ui-runtime/src/mutation.rs:672`)再提交,焦点因此与其他变异走同一提交边界。窗口失焦不清文档焦点("窗口 blur 故意保留文档的聚焦控件,回到窗口接着上次的位置输入",`SourceState::focus_ever` 注释);源断开时先结束 preedit 再清焦点,且仅当没有其他聚焦源。`cancel_source_pointers`(`crates/nana-ui-runtime/src/framework/input.rs:892`)合成 `PointerPhase::Cancel`,"组件听到的生命周期取消与显式取消完全一样,发生在捕获被撤销之前"。Tab 顺序遍历在 `navigate_sequential_focus`(`crates/nana-ui-runtime/src/framework/selection.rs:753`);WAI-ARIA 式工具栏/菜单方向键由 roving 组提供(`crates/nana-ui-runtime/src/framework/roving_focus.rs:1`),声明随节点走(`set_roving_focus_group :37`)。

### 5. IME 状态、HostServices 与无障碍投影

`HostServices`(`crates/nana-ui-input/src/host_services.rs:97`)把光标与 IME 明确定义为**状态**而非请求:"路由器只在状态变化时调用 set_cursor/set_text_input,宿主永远不会看到一排过期意图,满队列也永远不会拖住输入。"剪贴板是唯一带应答的请求,在事件循环线程同步执行,忙时返回 `Busy` 而不是等待。`TextInputContext.owner` 保证焦点换字段即换 IME 会话("旧字段的合成不得提交进新字段")。runtime 侧的 `SourceEffects`(`crates/nana-ui-runtime/src/framework/input/effects.rs:43`)是实现:光标与 text-input 都是"最新值槽位",派生、按廉价键比较、不同才发送;手柄探测以 `HANDLE_PROBE_INTERVAL = 8ms` 节流。`commit_ime`(`crates/nana-ui-runtime/src/framework/text_input.rs:81`)强调组件编辑器必须经组件提交:"只走世界路径会留下组件的旧值,下一次投影会把这次提交写没。"实现侧有三个配套:`UnsupportedHostServices`(`host_services.rs:114`)给不需要宿主能力的环境、`HeadlessHostServices`(`:134`)保留最后的光标/IME/剪贴板值供测试断言、原生窗口的 `NativeWindowServices`(见下文协作)把状态攒到 drain 末尾一次应用。

无障碍投影是输入链路的最后一环:`project_accessibility_delta`(`crates/nana-ui-runtime/src/world/accessibility.rs:630`)产出"一个完整的增量无障碍事务,含墓碑"。受影响集合 = 无障碍脏位 + 命中测试/变换子树的闭包——"滚动与变换不改变 LayoutBox……但它们的命中子树在视口空间里移动了,原生无障碍缓存必须看见"。而排期布局节点被有意不作种子:"布局失效会向祖先传播……而真正移动了的节点已经由另一条路到达——布局写回提交 WriteLayout,恰好把这些节点(含被移动的后代)标上 INPUT|RENDER|ACCESSIBILITY。"命中查询由 `world/hit_test.rs:1` 的增量命中索引承担,`hit_test`(`:1447`)在首个命中即返回,"指针分发在每个移动上不必收集再丢弃整个候选列表";滚动走 `update_hit_test_scroll`(`:1514`),把滚动平移预合成到后代命中项——"成员、顺序、z-index 与裁剪对滚动不变,所以等价于一次重建"。

### 6. 事件监听器、输入模态与观测

树上的事件监听器是 NodeStore 的一张稀疏侧表(`event_listeners`,`crates/nana-ui-runtime/src/store.rs:379` 的字段列表),由 `MutationQueue::set_event_listener`(`crates/nana-ui-runtime/src/mutation.rs:523`)写入——监听器的装订与其他变异走同一个提交边界。Vue 宿主侧对应的开关是 `set_event_flag`(`crates/nana-ui-vue/src/tree.rs:2712`,按事件名启停),渲染器在 `crates/nana-ui-vue/src/renderer.rs:1238` 调它;也就是说「`event_flags` 在 Runtime 事件监听器」这条 AGENTS.md 合同的落点是:vue 侧的 flag 最终落到 runtime 的 `event_listeners` 侧表,不存在第二套派发。

分发链还会在每次路由时记录**输入模态**(`dispatch_pointer`/`dispatch_keystroke` 开头的 `note_input_modality`),供工作台的自动隐藏 chrome 判断「用户在用鼠标还是键盘」(见[工作台层](/learn/workspace-shell-dock)的 `OverlayActivity`)。观测面分三层:`InputCounters`(`crates/nana-ui-runtime/src/framework/input.rs:146`)累计每 context 的路由统计:

| 计数器 | 含义 |
| --- | --- |
| `events_routed` / `events_rejected` | 成功路由 / 被验证拒绝的事件数 |
| `hover_changes` | 悬停目标变化次数 |
| `cursor_updates` | 发给宿主的光标状态变化 |
| `text_input_updates` | text-input(IME)状态变化 |
| `text_suppressed` | 因 `handled_key` 被丢弃的提交文本 |

`nana_diagnostics` 计量覆盖 `INPUT_EVENTS`/`INPUT_COALESCED`/`INPUT_PAYLOAD_BYTES`(端点侧)与 `INPUT_HIT_TESTS`/`INPUT_FOCUS_CHANGES`/`INPUT_CAPTURE_CHANGES`/`INPUT_HOVER_CHANGES`/`INPUT_ROUTE_NS`(路由侧);宿主的程序观察用 `route_next`(`framework/input.rs:451`)一次一事件地走,程序对按键的处理(如 `suppress_text_for`,`:469`,页面版 `keydown.preventDefault`)恰好落在它命名的文本之前。

## 协作与数据流

一次原生输入的完整旅程:

1. winit 事件进入 `handle_window_event`(`crates/nana-ui/src/scene_host/input.rs:54`),同时喂给平台无障碍桥;模态子窗口会门控父窗口事件。
2. `push_input`(`:475`)用源的 `InputSequencer` 盖章,推入 `InputEndpoint`;端点满则先 drain 再重试,仍满报 `HostFailure::InputDispatch`。
3. `drain_window_input`(`:509`)是"每条原生输入路径都要过的那一次 drain":先 `bind_input_source`;若程序给窗口换了文档(`DocumentRebind`),规范信封里没有文档 id,旧事件绝不重放进新文档——全部弹出、解绑、重绑。
4. `AppContext::route_input` 验证、本地化、分发;副作用通过 `MutationQueue` 提交进 `UiWorld`,`flush_reactive()` 立即结算信号绑定。
5. 帧边界由 `RuntimeDocument::flush` 排水系统(见[运行时保留树](/learn/runtime-tree));宿主随后 `refresh_input_effects` 同步光标/IME/悬停。
6. 光标与 IME 状态由 `NativeWindowServices`(`crates/nana-ui/src/scene_host/host_services.rs:87`)在 drain 末尾一次性应用到窗口;剪贴板走进程唯一的 `OnceLock` 后端。

无头与测试路径由 `HeadlessInput` 提供:`route_shaped`(`crates/nana-ui-runtime/src/framework/input/headless.rs:119`)盖章后直接进 `route_input`;`press`(`:145`)按"键盘的交付方式"先发键、再发以其命名的文本;时间只在被要求时前进,"脚本化会话保持可复现"。Vue 宿主把 JS 采样 `to_canonical()` 后在 `crates/nana-ui-vue/src/host/input_projection.rs:386` 走同一条路由——两个宿主,一棵树,一条输入链。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-input/src/canonical.rs` | 规范事件、身份类型、序列器、端点与合并 |
| `crates/nana-ui-input/src/host_services.rs` | `HostServices` 合同、光标枚举、IME 上下文、无头实现 |
| `crates/nana-ui-input/src/input.rs` | `InputModifiers`/`PointerPhase`/`PointerType`/`InputDisposition` |
| `crates/nana-ui-runtime/src/framework/input.rs` | 路由器:绑定、验证、本地化、焦点路由、断开处理 |
| `crates/nana-ui-runtime/src/framework/input/dispatch.rs` | 指针/滚轮/按键/合成四条分发链 |
| `crates/nana-ui-runtime/src/framework/input/effects.rs` | 光标与 text-input 最新值槽、手柄探测节流 |
| `crates/nana-ui-runtime/src/framework/input/headless.rs` | `HeadlessInput` 测试输入源 |
| `crates/nana-ui-runtime/src/framework/text_input.rs` | IME preedit/commit/surrounding 世界路径 |
| `crates/nana-ui-runtime/src/world/hit_test.rs` | 增量命中索引与变换感知命中查询 |
| `crates/nana-ui-runtime/src/world/accessibility.rs` | 无障碍增量投影与墓碑 |
| `crates/nana-ui/src/scene_host/input.rs` | winit 事件入队、drain、重绑语义 |
| `crates/nana-ui/src/scene_host/host_services.rs` | 原生窗口的 `HostServices` 实现与端点容量 |

## 延伸阅读

- [运行时保留树](/learn/runtime-tree):事件副作用如何经 `MutationQueue` 进入提交边界,帧内系统如何排水。
- [布局引擎](/learn/runtime-layout):命中测试与无障碍投影消费的 `LayoutResult` 从哪里来。
- [窗口与平台层](/learn/window-platform):原生窗口事件如何到达 `handle_window_event`。
- [框架合同](/architecture/frame):输入合同在整个架构中的位置。
- [合同与不变量](/learn/contracts-and-invariants):跨层合同清单里输入相关的条目。
