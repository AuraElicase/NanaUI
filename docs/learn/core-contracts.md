---
title: 核心合同:nana-ui-core
---

# 核心合同:nana-ui-core

这一章下钻整个框架的地基 `nana-ui-core`:它定义的共享合同——Style Model(Tokens / Semantics / Layout 三层)、主题栈、几何原语、Workspace 合同、Motion 与 Motion IR——以及「为什么这些东西要放在最底层」。读完你能搞懂:一条样式从 CSS 或 Vue 属性变成 `LayoutStyle` 字段时落在哪层;语义调色板如何既服务深浅主题又守住 WCAG 对比度;`WorkspaceModel` 如何在不依赖窗口后端的前提下管理区域布局与折叠动画;Motion 为什么只允许一条时间线权威。它承接[模块与分层](/learn/modules-and-layers)的层位结论,是 [CSS 引擎](/learn/css-engine)与[文本引擎](/learn/text-engine)两章的共同前置。

## 职责与边界

`nana-ui-core` 的文件头一句话定调:"Backend-neutral NanaUI contracts shared by Runtime and Vue adapters"(`crates/nana-ui-core/src/lib.rs:1`)。它是后端中立的合同层:所有样式路径汇入同一个模型——Style Model,**Tokens + Semantics + Layout**;L1 CSS 适配器与 L2 Vue 属性*映射进*这个模型,L3 Rust API 是原生入口;绘制留在 Runtime / UiScene(`crates/nana-ui-core/src/lib.rs:5-7`)。

- **共享,而非实现**:core 定义「样式数据长什么样」「主题如何编译」「区域如何声明」,不解析任何输入。文件头明确:"This crate must **not** depend on CSS parsers, JS engines, WebView, or window backends"(`crates/nana-ui-core/src/lib.rs:8-9`)。
- **零 CSS 引擎的边界**:CSS 解析整个被挡在 core 之外,`style_model.rs` 的非目标清单写明 CSS 解析/CSSOM 不得进入本 crate(`crates/nana-ui-core/src/style_model.rs:43-46`)。core 与 CSS 唯一的接触点是 `SemanticColorRole::from_css_token_name`(`crates/nana-ui-core/src/style_model.rs:341`):它消费的是 token *名字*(`--nana-*` 变量名),不解析任何 CSS——未知 `#hex`/`rgb()` 直接返回 `None`。
- **双宿主中立**:Rust Runtime 与 Vue 适配器(乃至跨语言宿主)都要产出同一份样式数据,所以合同必须比任何一方都低层;文本、CSS、图表、输入、窗口、GPU 六个 L2 crate 都只依赖 core + 外部库。
- 绘制、布局求解、输入路由都在上层;core 只保证各方对同一份数据的*口径*一致。
- 诊断走 `nana-diagnostics` 的静态描述符合同,core 不引日志 crate——这也是它仅有的三个依赖之一(见下节)。

## 依赖与邻居

`crates/nana-ui-core/Cargo.toml` 的依赖表只有三项:`nana-diagnostics`(框架诊断合同)、`serde`、`serde_json`——没有 CSS 解析器、没有窗口库、没有 GPU 类型。这使它稳居 L1(只依赖外部库):

| 邻居 | 关系 |
| --- | --- |
| `nana-text` / `nana-ui-css` / `nana-ui-charts` / `nana-ui-input` / `nana-window` / `nana-gpu` | 六个 L2 crate,只依赖 core + 外部库;排版词汇(`LineBreakSpec`、`WritingModeSpec` 等)从 core 借用 |
| `nana-ui-runtime` | 在 core 之上建保留树;`NodeStyle` 内含 `LayoutStyle`,主题经 `CompiledTheme` 编译后挂树 |
| `nana-ui-css` | 唯一 CSS 引擎,把声明映射到 core 的 `LayoutStyle`(见[内部结构](#内部结构)) |
| `nana-ui-vue` | JS/TS 宿主侧适配器,把 Vue 属性同样映射进 Style Model;CSS 解析在这层的 `css_map` 完成 |
| `nana-diagnostics` | `event!` / `metric!` / `fault!` 静态描述符,core 是最早的消费方之一 |

## 关键类型与数据

| 类型 | 所在文件 | 作用 |
| --- | --- | --- |
| `StyleModelRef` | `crates/nana-ui-core/src/style_model.rs:839` | 每节点样式读取句柄:theme_appearance / metrics / palette / titlebar / on_media / opacity,注释明言 "small on purpose" |
| `SemanticColor` | `crates/nana-ui-core/src/style_model.rs:60` | RGBA、`f32` 分量 0..=1 的语义色 |
| `PaintColor` | `crates/nana-ui-core/src/style_model.rs:70` | 作者空间绘制色:`Srgb` / `Oklch` / `Hsv` / `LinearScRgb`,带 `to_srgb` / `to_linear_sc_rgb` 换算 |
| `PaintColorSlots` | `crates/nana-ui-core/src/style_model.rs:165` | `LayoutStyle` 上的颜色槽位:color/background/border(含四边)、outline、selection_* |
| `SemanticColorMix` | `crates/nana-ui-core/src/style_model.rs:393` | 基点权重的语义色混合,`resolve()` 预乘 alpha |
| `SemanticColorRole` | `crates/nana-ui-core/src/style_model.rs:257` | ~45 个语义角色(含代码 token 角色 Function/Builtin/Type/Variable…),`from_css_token_name` 在 `:341` |
| `SemanticPalette` | `crates/nana-ui-core/src/style_model.rs:456` | 语义调色板:`dark()`/`light()`/`high_contrast()`/`for_system_contrast()` 等一族的来源 |
| `LayoutStyle` | `crates/nana-ui-core/src/box_layout.rs:3292` | Style Model 的 Layout 盒级切片:flex/gap/padding/变换/书写方向等全部字段 |
| `LengthSpec` | `crates/nana-ui-core/src/box_layout.rs` | 长度意图:`Px` / 百分比 / `calc` 表达式等,布局时才对包含块求解 |
| `RegionState` / `WorkspaceLayout` | `crates/nana-ui-core/src/layout.rs:94` / `:386` | Workspace 区域合同:placement/scope/尺寸边界/折叠行为 |
| `WorkspaceModel` | `crates/nana-ui-core/src/workspace_model.rs:85` | 区域布局权威:持久区域表 + 缩放交互 + 视口事实 + 折叠过渡 |
| `MotionTrack` / `MotionGraph` | `crates/nana-ui-core/src/motion/ir.rs` | 绝对时间动作轨道;创作辅助(sequence/parallel)编译到这里 |
| `ThemeDefinition` / `CompiledTheme` / `ThemeRegistry` | `crates/nana-ui-core/src/theme/` | 主题定义 → 编译产物 → 注册与解析 |
| `LogicalRect` / `LogicalPoint` | `crates/nana-ui-core/src/geometry.rs:22` | 逻辑像素几何原语(区别于场景层物理像素) |
| `AccentRamp` | `crates/nana-ui-core/src/theme/tokens.rs:42` | 强调色一族:base/strong/on_soft/text + soft 系 alpha + focus(写定而非推导) |
| `MotionTokens` / `MotionRole` | `crates/nana-ui-core/src/theme/tokens.rs` | 动效时长/缓动的 token 类别;`DEFAULT_MOTION` 即 `MotionTokens::DEFAULT` |
| `ContentFit` | `crates/nana-ui-core/src/geometry.rs:41` | CSS object-fit 语义:Fill/Contain/Cover/None/ScaleDown |
| `ControlSemantics` | `crates/nana-ui-core/src/style_model.rs:914` | 语义层控制点:size / button_kind / card_kind / status |

## 内部结构

### Style Model:Tokens + Semantics + Layout

三层各回答一个问题:`style_model.rs` 的模块头给出落层规则(`crates/nana-ui-core/src/style_model.rs:19-24`):

| 层 | 内容 | 谁写 |
| --- | --- | --- |
| Tokens | `ThemeMetrics` / `ThemeAppearance` / `SemanticPalette`(`crates/nana-ui-core/src/theme/`) | 主题档;组件不越权发明 token |
| Semantics | `crate::semantics`:`ButtonKind`、`ControlSize` 等组件语义 | 已知类名(如 `nana-btn--primary`)归这层,不新造 token |
| Layout | `crate::layout`(Workspace 区域)+ `crate::LayoutStyle`(盒级弹性,`box_layout.rs`) | flex/gap/padding/尺寸声明 |

配套的 L1 颜色政策(`crates/nana-ui-core/src/style_model.rs:26-30`):已知 token/类名映射到 `SemanticColorRole`;未知的 `#hex`/`rgb()` *不得*进正式 `ThemeTokens`,只能作为受限绘制提示留在桥接层或丢弃。这保证业务 CSS 任意颜色不会反向污染主题体系——主题是唯一的色彩权威,层级在[契约与不变量](/learn/contracts-and-invariants)一章展开。

两层 Layout 切片在 `layout.rs` 头部被明确区分:"Workspace region layout contract (Style Model Layout — shell regions). Distinct from box flex layout (`crate::LayoutStyle` in box_layout). Both are Layout-slice inputs to Nana drawing; neither is a CSSOM"(`crates/nana-ui-core/src/layout.rs:1-8`)。

### 语义色与调色板

`SemanticPalette` 提供 `dark()`(:501)、`light()`(:541)、`high_contrast()`(:593)、`high_contrast_light()`(:635)、`for_system_contrast()`(:678)一族构造器,`with_accent_ramp()`(:697)在换强调色时重导 accent_soft 家族,让填充与标签永远协调。取色入口 `get_in(role, OpacityTokens)`(:760)内置大量系统级约定:WarningSoft/DangerSoft 用主题 alpha 的 StateLayer、Titlebar 落到 surface、OnMedia 用 `MEDIA_FOREGROUND`;代码 token 角色默认值如 Keyword=accent_strong、Function/Builtin=accent、Type=accent_on_soft(:824-832)。

两个细节值得记住:

- **可混合**:`SemanticColorMix`(:393)用基点权重做预乘 alpha 混合,`resolve()` 是唯一出口。
- **对比度是测试保证的**:测试注释里写着焦点面 ≥3:1(WCAG 2.4.11)、焦点文本 ≥4.5:1(1.4.3)及实测比值(深色下 5.80:1)。强调色焦点的推导理由在 `AccentRamp` 字段注释里:`focus` 直接写定而非推导,因为深色底 `#181818` 上 0.50 alpha 能到 3.16:1,而白底上任何 alpha 都到不了 3:1(`crates/nana-ui-core/src/theme/tokens.rs:42-69`)。

### 主题栈:ThemeDefinition → CompiledTheme → ThemeRegistry

主题侧的原则写在 `theme/tokens.rs` 头部:"the component names a step, the theme owns the value"——组件只报枚举,值归主题(`crates/nana-ui-core/src/theme/tokens.rs:1-10`)。查询是「对枚举 match」而非字符串哈希,Issue #100 明令禁止运行时路径上的 `theme.get("radius.medium")`;裸常量(`space`、`type_scale`、`HAIRLINE`)全部从 `DEFAULT` 结构体推导,`scripts/audit-theme-hardcoding.py` 盯着第二权威出现。

- `theme/mod.rs` 汇出编译好的 `CompiledTheme`/`builtin_theme`,注册与解析走 `ThemeRegistry`(ThemeChoice/ThemeResolution),`UI_BASE_TEXT_SIZE` 锚定在 `TypographyTokens::DEFAULT.body`(`crates/nana-ui-core/src/theme/mod.rs:38`)。
- `mod space`(:81-111)的间距刻度 XXS..XXXL 全部由 `SpacingTokens::DEFAULT` 推导,注释强调 "not a second authority";8 和 6 是组件树用得最多的两档。
- `ThemeAppearance` 只有 `Light`/`Dark`(默认)/`Custom` 三态(:41-47),`palette()` 按外观取 `SemanticPalette`(:63)。

### 几何原语与 Workspace 合同

`geometry.rs` 定义逻辑像素原语与少量窗口几何常量:`LogicalPoint`(:22)、`LogicalRect`(:99,构造时把负宽高钳为 0)、`ContentFit`(Fill/Contain/Cover/None/ScaleDown,:41-53,即 CSS object-fit 在内容盒上的语义,绘制端采样加裁剪、不额外拷贝 GPU 纹理)、`ImageSampling`(Resample/Mipmap,:89-95)。标题栏常量(`TITLE_BAR_HEIGHT = 36.0` 等)与 `custom_window_controls_width()`(:16)也在这一层,供窗口层与绘制层共用。

Workspace 合同分两半:

- **静态结构**(`crates/nana-ui-core/src/layout.rs`):`RegionId`(GlobalNavigation/SectionNavigation/Resources/PrimaryToolbar/Primary/Inspector/Diagnostics/Custom,:15-24)、`RegionRole`、`RegionPlacement`、`RegionScope`、`NarrowBehavior`(Shrink/Collapse/Overlay)、`RegionState`(:94,含 min/max/default 尺寸、fill_priority、collapse_below)。
- **交互与过渡**(`crates/nana-ui-core/src/workspace_model.rs`):`WorkspaceModel` 的注释说它 "Owns persisted region layout, resize interaction, viewport facts and collapse transitions without depending on a window or renderer backend"(:82)。变异走 `WorkspaceMutation` 枚举(:13-26,ToggleRegion/ResizeMove/SetViewport/…),折叠动画是 `RegionTransition`(:44),`extent_at(now)` 用 `Easing::EaseInOutCubic` 求当前尺寸(:61-69),时长锚定 `WORKSPACE_REGION_TRANSITION_DURATION`(:10)。

### LayoutStyle:盒级弹性布局意图

`box_layout.rs` 文件头自我定位:"Style Model Layout — box flex intent (not a CSSOM, not workspace regions). Pure data consumed by L3 draw … and L1/L2 adapters. CSS declaration/class parsing stays in nana-ui-vue::css_map"(`crates/nana-ui-core/src/box_layout.rs:1-6`)。`LayoutStyle` 结构体在 `crates/nana-ui-core/src/box_layout.rs:3292`,字段覆盖 `paint_colors`、`direction`/`dir`/`writing_mode`/`text_orientation`、`flex_reverse`/`order`/`flex_wrap`、`display`/`position`/`z_index`、`transform`(绘制级 2D,不参与 flex/grid 测量)、`gap`/`padding`/`margin`(TRBL + 逻辑边)等。

书写方向词汇也在 core:`DirSpec`(:48)只重映射逻辑盒边与 text-align 的 start/end,不翻转 flex 主轴;`WritingModeSpec`(:65)不含 `sideways-*`,解析时直接跳过、失败即收(fail-closed);`TextOrientationSpec`(:87)按 UAX #50 取向。`AlignSpec::Baseline` 的基线来自 Runtime 共享的内在度量(:104)。

### Motion / Motion IR:唯一时间线权威

`motion/mod.rs` 头部声明:"This module is the only Motion timeline authority"(`crates/nana-ui-core/src/motion/mod.rs:1-3`):Runtime 的 `AnimationSpec` 只是时序/播放子集,采样统一走 `evaluate_progress`;Vue/CSS 编译器必须产出 `MotionTrack`/`MotionGraph`,不得自建时钟;`PresentationStore` 是这些轨道之上的瞬态覆盖层,不是第二条时间线。IR 本体在 `crates/nana-ui-core/src/motion/ir.rs`:`MotionTrackId`(:18,同 ID 重启即原子替换)、`MotionTargetId`(:32,Runtime 映射 `StableNodeId`)、`MotionValue`(:46,Scalar/Color/Transform/Discrete)。插值 `MotionValue::lerp`(:93)刻意不把缓动进度钳回 0..=1——钳掉 overshoot 会毁掉 back/anticipation 曲线;颜色是唯一按通道钳制的例外。

`DEFAULT_MOTION` 就是 `MotionTokens::DEFAULT`(:84,又一次 "not a second authority");`HOVER_COLOR`/`OVERLAY_FADE`/`SIDEBAR_COLLAPSE` 等时长常量(:87-99)全部从主题角色推导,能问 `theme.duration(MotionRole::…)` 的调用点应该去问。子模块各有分工(`crates/nana-ui-core/src/motion/mod.rs:1-14`):`eval` 提供 `evaluate_progress`/`evaluate_track`/`retarget_track`(CPU 求值唯一入口);`descriptor` 把合成器安全轨道编进 `MotionDescriptor`(带版本 `MOTION_DESCRIPTOR_VERSION` 的世代化 slab);`presentation` 的 `PresentationStore` 只做瞬态覆盖;`codec` 负责编码;`gpu` 负责存储缓冲打包;`inspector` 出计数。

### 为什么这些要抽到最底层

把 Style Model、主题、几何、Workspace、Motion 放在 L1,不是洁癖,而是四个硬约束:

- **双宿主同一份数据**:Rust Runtime 与 Vue 适配器要产出可逐字段对比的样式;合同层必须比任何一方都低(`crates/nana-ui-core/src/lib.rs:1`)。CSS 引擎的两个消费时机(构建时 codegen、运行时 vue)之所以能承诺「同一批匹配声明产出相同 `LayoutStyle`」,前提是目的地模型只有一份。
- **菱形依赖的底**:text/css/charts/input/window/gpu 六个 L2 各自只依赖 core + 外部库;若任何一个合同散落在 runtime,六个 crate 就得互相依赖或复制定义。
- **词汇不重造**:文本引擎的排版枚举(`LineBreakSpec`、`WritingModeSpec`…)只能从 core 借用、白名单封口(`crates/nana-text/src/lib.rs:30-37`),UiWorld 适配才是逐字段搬运;书写方向在 `LayoutStyle`(`DirSpec` 只重映射逻辑边)与文本约束里必须同义。
- **失效要便宜**:core 上的数据要参与「写时解析一次、读者克隆 `Arc`」这类运行时策略,它自己必须零解析、零窗口、零 GPU——所以 lib.rs 把禁令写在文件头(`crates/nana-ui-core/src/lib.rs:8-9`)。

## 协作与数据流

两个生产端汇到一个模型:

1. **CSS 端**(构建时与运行时):`.vue` 编译器在构建期解析并匹配,产出数据补丁;Vue 运行时用同一引擎现场级联。两端写的是同一个 `LayoutStyle`(详见[CSS 引擎](/learn/css-engine))。
2. **Rust 端**:组件 API 直接填 `LayoutStyle` 字段与语义(`ControlSemantics`),不经过任何字符串。

以「一张卡片的样式」为例走一遍:

1. **入口分流**:Vue 侧 `nana-ui-vue` 的 `css_map` / Rust 侧组件 API,把 CSS 声明或属性写进 `LayoutStyle` 字段——注意 CSS 解析发生在消费者,core 只收数据。
2. **主题解析**:运行时装主题得到 `CompiledTheme`;`StyleModelRef` 按节点暴露调色板/度量/外观,`color()`(:892)处理 Titlebar/OnMedia 覆盖,`base_text_size()` 回到 `UI_BASE_TEXT_SIZE`。
3. **消费**:Runtime 布局求解读 `LayoutStyle` + Workspace 区域表;绘制读 `PaintColorSlots`;Motion 轨道经 `evaluate_track` 求值,合成器安全轨道另存 `MotionDescriptor`(世代化 slab),瞬态样本不回写基础样式(见[契约与不变量](/learn/contracts-and-invariants))。
4. **失效**:样式变化经 `LayoutStyle` diff 驱动局部重排;主题切换换掉整个 `StyleModelRef` 背后的调色板,`with_accent_ramp` 保证强调色家族一致。
5. **文本与图形汇入**:文本节点测量时,`LayoutStyle` 里的书写方向/字号语义翻成 `nana_text` 的 `TextStyle`/`TextConstraints`;绘制端读 `PaintColorSlots` 与 `ContentFit` 决定采样与裁剪——两者消费的都是本章合同,不经过运行时私造的中间格式。

## 代码地图

| 文件 | 内容 |
| --- | --- |
| `crates/nana-ui-core/src/style_model.rs` | Style Model 总纲:三层落层规则、语义色/角色/调色板、`StyleModelRef` |
| `crates/nana-ui-core/src/theme/` | tokens.rs(类型化 token 类别)、definition.rs、compiled.rs、registry.rs、recipe.rs(组件配方) |
| `crates/nana-ui-core/src/box_layout.rs` | `LayoutStyle` 与盒级弹性词汇(6300 行,Style Model 的 Layout 主体) |
| `crates/nana-ui-core/src/layout.rs` | Workspace 区域合同(与 box flex 的分工写在文件头) |
| `crates/nana-ui-core/src/workspace_model.rs` | 区域布局交互权威与折叠过渡 |
| `crates/nana-ui-core/src/geometry.rs` | 逻辑几何原语、ContentFit、窗口几何常量 |
| `crates/nana-ui-core/src/motion/` | ir.rs(Motion IR)、easing、eval、descriptor、presentation、gpu(打包) |

## 延伸阅读

- [模块与分层](/learn/modules-and-layers):core 在依赖图里的 L1 位置与 L2 邻居全景
- [CSS 引擎](/learn/css-engine):`nana-ui-css` 如何把声明映射到本章的 `LayoutStyle`
- [文本引擎](/learn/text-engine):`nana-text` 从 core 借用的排版词汇清单
- [契约与不变量](/learn/contracts-and-invariants):「主题是唯一色彩权威」「瞬态样本不回写」等不变量的执行面
- [参考:架构](/reference/architecture):Style Model / Motion IR 的合同级描述
