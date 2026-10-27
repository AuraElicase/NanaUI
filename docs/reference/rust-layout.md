# 布局与样式（Rust 路径）

本文说明在 Rust L3 路径中如何排列容器、设置语义样式、控制边框与处理容器内边距。如果你使用 `.vue` 或构建时 `<style>` / `stylesheet!` 编写 CSS，支持的 CSS 属性子集请查阅 [布局（CSS 子集）](layout.md)。

## 优先使用 `Stack` 预设，避免手写底层布局字段

[`Stack`](../../crates/nana-ui-runtime/src/view_components.rs) 是 Rust 路径的标准布局容器。常用排列直接使用预设构造器（声明式视图中对应 `<Column>` / `<Row>` 或 `column()` / `row()`）；需要 Grid、定位、溢出或绘制属性时，通过 `with_layout` 或 `from_layout` 写入同一份 [`LayoutStyle`](../../crates/nana-ui-core/src/box_layout.rs)，无需另外定义新的布局组件。

Row/Column 预设直接写入 `display: flex` 与方向，进入同一个 Flex formatting context；它们不建立另一种布局身份。`from_layout` 保留传入的 display，Grid 也只通过同一份布局意图表达。Rust、`view!` 和构建期 `.vue` 方言共用这条 Runtime 路径；比较等价 CSS 时，需同时写明预设的尺寸与对齐默认值。

```rust
use nana_ui::runtime::{JustifySpec, SemanticColorRole, Stack};

// 工具条：水平排列、宽度随内容收缩、子项垂直居中；默认起点对齐，右对齐用 JustifySpec::End
let toolbar = Stack::row(8.0).justify(JustifySpec::End);

// 窄宽度下自动换行的操作区
let actions = Stack::bar(8.0).wrap(true);

// 主内容区：占满父容器剩余高度
let body = Stack::fill_column(12.0);

// 带描边的卡片容器：一次性设置背景、边框角色与线宽、圆角
let card = Stack::column(7.0)
    .padding(16.0)
    .surface(SemanticColorRole::Surface)
    .outline(SemanticColorRole::Border, 1.0)
    .radius(16.0);
```

`with_layout` 闭包拿到的 `&mut LayoutStyle` 上有几个就地写的简写，写的仍是 CSS 作者会写的那几个字段：`set_absolute_inset(px)`（`position: absolute` 加四边 `inset`）、`set_absolute_fill()`（四边为 0，盖住包含块）、`set_fixed_width(px)` / `set_fixed_height(px)`（尺寸与最小、最大同值，不随内容或 flex 伸缩）。它们返回 `&mut LayoutStyle`，可以连写。手上是 `NodeStyle` 时用 `NodeStyle::layout_mut()` 取可写布局（共享的默认布局会先复制，不会被写穿），不必自己 `Arc::make_mut`。

布局结果 `LayoutBox` 带 `right()`、`bottom()`、`union`、`intersection`（不相交为 `None`）和 `inset(amount)`（四边内收，收过头时在中点缩成 0），算浮层、安全区和命中范围时直接用。

子节点在声明式视图中通过 `.children((...))` 或 `.with(|c| c.add(...))` 挂入容器（见 [L3：用 Rust 构建保留式界面](l3-authoring.md)）。`Stack` 只负责纯排版；具备列表或表格语义的容器请使用 [`List`](../components/list.md) 或 [`Table`](../components/table.md)。

| 预设构造器 | 主轴方向 | 尺寸行为 | 典型用途 |
| --- | --- | --- | --- |
| `Stack::row(gap)` | 水平 | 宽度随内容收缩，交叉轴居中 | 工具条、按钮组（默认起点对齐，`.justify(End)` 右对齐） |
| `Stack::fill_row(gap)` | 水平 | 占满父级剩余宽度 | 整行多栏分段 |
| `Stack::column(gap)` | 垂直 | 高度随内容收缩，宽度占满父级 | 页面纵向堆叠 |
| `Stack::fill_column(gap)` | 垂直 | 占满父级剩余高度与宽度 | 主内容区、带贴底操作栏的面板 |
| `Stack::bar(gap)` | 水平 | 占满整行宽度，子项不自动拉伸 | 顶栏、底栏、状态条 |

::: tip 常见排错：`column` 与 `fill_column`
如果发现主内容区没有撑开、或者底部状态栏/输入框没有贴到窗口底边，几乎都是因为本该使用 `Stack::fill_column` 的容器写成了随内容收缩的 `Stack::column`。
:::

Rust 预设的 `Stack` 容器默认不参与指针命中测试（`pointer_events: false`），不会拦截下层点击；而承接已解析 `LayoutStyle` 的 `Stack::from_layout` 默认可命中。

## 节点样式：`NodeStyle` 与边框规则

控件通过 `.style(NodeStyle)` 接收自定义样式。颜色一律使用 [`SemanticColorRole`](../../crates/nana-ui-core/src/semantics.rs) 语义角色，切勿硬编码裸 RGBA 值（原因见 [视觉与主题规范](look.md)）：

```rust
use nana_ui::runtime::{NodeStyle, SemanticColorRole};

let style = NodeStyle::default()
    .surface(SemanticColorRole::Surface)      // 背景表面色
    .outline(SemanticColorRole::Border, 1.0) // 边框：颜色角色 + 线宽一次写全
    .radius(8.0);
```

### 边框必须同时具备颜色与宽度

底层将边框拆分为两处存储：颜色位于 `NodeStyle.border`（语义角色）或 `LayoutStyle.border_color`，线宽位于 `LayoutStyle.border_width`。**只要任意一方缺省，边框就完全不会绘制**（编译期与运行期均不会报警）。因此：

- **绘制边框**：始终调用 `NodeStyle::outline(role, width)` 或 `Stack::outline(role, width)` 一次性写全颜色与线宽，不要只改其中一个字段。
- **关闭边框**：将 `border_width` 设为 `Some(0.0)`，同时清空 `border` 以及交互态（hover / focus）的边框角色。
- **自带外观的卡片（`Card`）**：需要 `1 px` 描边时直接使用 `Card::new().kind(CardKind::Outlined)`，不必手动叠加 `NodeStyle`。通过 `.style(...)` 显式指定的背景、边框和圆角优先于 `CardKind` 的默认值。

### 随主题解析的语义混色（`SemanticColorMix`）

当需要在两个语义色之间按比例混合、或只降低背景/边框自身的不透明度时，使用 `SemanticColorMix`：

- `SemanticColorMix::new(first_role, second_role, first_weight)`：在预乘 alpha 的 sRGB 空间对当前主题的两种语义色做混合（`first_weight` 取值 `0.0..=1.0`，精度至万分之一；切换主题时自动按新调色板重新解析）。
- `SemanticColorMix::alpha(role, alpha)`：仅降低该表面或描边颜色自身的 alpha，**不会降低后代文本的不透明度**。
- 通过 `NodeStyle::surface_mix(mix)` 与 `outline_mix(mix, width)` 设置基态混色（与 `surface` / `outline` 后调用者生效；`Stack` 上同名方法直接可用）；在 `bind` 闭包里改容器布局用 `Stack::layout_mut()`，不必整个替换容器；`InteractionStyle` 的各状态（`base` → `selected` → `hovered` → `pressed` → `focused` → `disabled`）可按优先级覆盖基态。

## 与 Web CSS 默认值的关键差异

`LayoutStyle` 的字段命名对齐 CSS，但其缺省值是**显式收敛（fail-closed）**的，直接套用浏览器 CSS 的 initial 值假设会出错：

1. **`flex-shrink` 缺省为 `0`（浏览器 CSS 为 `1`）**：定宽行溢出时子项保持原有尺寸，不会被静默压扁；需要随容器收缩时必须显式调用 `.shrink(1.0)`。
2. **`align-items` 缺省为 `Start`（浏览器 CSS 为 `stretch`）**：子项不会自动沿交叉轴拉伸铺满（`Stack` 预设已按常见用途配好对齐方式，手动构造 `LayoutStyle` 时需留意）。
3. **`flex-direction` 缺省为 `Column`（浏览器 CSS 为 `row`）**：未指定方向的容器默认自上而下竖排，需要水平排列请显式使用 `Stack::row`。

与 CSS Flexbox 保持一致的关键行为：一行中的子项会按该行实际分配给它的**主轴使用尺寸（used main size）**去排版内部内容并测量交叉轴高度。例如当 `Fill` 列在定宽侧栏旁被收窄时，其内部的 16:9 缩略图与自动换行文本都会按收窄后的宽度重新算出真实高度，绝不会残留按整窗宽度量出的旧高度（除非子项自身显式锁死了交叉轴 `height` 或自带 `aspect_ratio`）。

## 浮层定位与容器内边距归属

模态对话框、下拉菜单、抽屉与气泡提示必须使用内置浮层控件（`Dialog`、`ActionMenu`、`Popover`、`Drawer`）并锚定到触发槽位，**不要手写 `position: absolute` 或 `fixed` 去拼装浮层**。框架内部的 `PositionSpec::Fixed` 以窗口视口为坐标基准，脱离外层滚动、变换与裁剪链，但仍保留在原 Runtime 树中并遵守结构性透明度与叠放顺序。

各层容器的内边距职责划分如下（框架不会根据嵌套层级自动清零内边距，兄弟间距优先使用父容器的 `gap`）：

| 容器 | 默认间距职责 |
| --- | --- |
| `AppShell` / `Workspace` | 管理区域切分与窗口 chrome，不替业务内容添加页面留白 |
| `Stack`（`Column` / `Row`） | 仅负责子项排列与显式 `gap`，默认 `padding` 为 `0` |
| `Card` / `SettingsCard` | 负责卡片内部留白（左右 `16 px`、上下 `14 px`），不附带外部 `margin` |
| `SettingsPage` | 负责内部可滚动正文的页面留白（上 `20`、右 `24`、下 `24`、左 `24 px`，标题与内容 `gap` 为 `16 px`） |

```rust
use nana_ui::runtime::{Card, Stack};
use nana_ui_core::PaddingSpec;

// 1. 页面中排列多张卡片：用外层 Stack::column(16.0) 控制卡片间距，Card 自带内部留白
let sections = Stack::column(16.0);
let card = Card::new();

// 2. 卡片内排列字段：Card 已提供内边距，内部 Stack 只管 gap，不再叠第二层 padding
let fields = Stack::column(8.0);

// 3. 贴边列表：哪一层容器需要贴边，就显式将该层的 padding 设为 0
let flush_card = Card::new().padding(0.0);
let flush_page = page.content_padding(PaddingSpec::uniform(0.0));
```
