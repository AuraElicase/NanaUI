# Stack

`Stack` 负责把若干子节点排成一行、一列，或叠成几层。行和列是 flex，叠层是自己的排版方式。尺寸、对齐和溢出的完整字段在 `LayoutStyle` 上。

## 基本用法

控件表里没有 `<Stack>`。视图函数 `column()` 是 `widget(Stack::column(0.0))`，`row()` 是 `widget(Stack::row(0.0))`。间距不为 0 时直接构造。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::Stack;

view! {
    <Widget of={Stack::row(8.0)}>
        <Text>"标题"</Text>
        <Widget of={Stack::spacer()} />
        <Button>"保存"</Button>
    </Widget>
}
```

```rust rust
use nana_ui::runtime::view::{button, text, widget};
use nana_ui::runtime::Stack;

widget(Stack::row(8.0)).children((
    text("标题"),
    widget(Stack::spacer()),
    button("保存"),
))
```

:::

## 预设

`Stack::row(gap)` 水平排列，宽度随内容，子项垂直居中，适合工具条和按钮组。

`Stack::fill_row(gap)` 水平占满父级剩余宽度，子项可以收缩。

`Stack::column(gap)` 竖直排列，高度随内容，宽度占满父级。

`Stack::fill_column(gap)` 竖直占满剩余高度，用来做主内容区。

`Stack::bar(gap)` 水平占满整行，自己不伸展，用来做顶栏和底栏。

`Stack::spacer()` 是零宽 flex-grow，把排在它后面的兄弟推到行尾。基准宽度是 0，否则它会铺满，把兄弟挤出去。

## 叠层

`Stack::layers()` 让所有子项共用同一块区域，后写的盖在先写的上面，和 SwiftUI 的 `ZStack`、Compose 的 `Box`、Flutter 的 `Stack` 一样。

- 容器随最大的子项。给了宽高时，这块区域就是容器的内容盒。
- 每个子项在这块区域里对齐：`.align` 管竖直，`.justify` 管水平，默认左上。子项自己的 `align-self` / `justify-self` 优先，`stretch` 铺满整块区域。水平方向的起点跟随书写方向，RTL 下是右边。
- 绝对定位的子项照常按包含块定位，不撑大容器。
- 命中和绘制都按文档顺序，重叠处由后写的那层接住指针。

CSS 写作 `display: layers`，这是 NanaUI 的扩展值，对齐用 `justify-items` / `align-items` / `place-items`。它按网格排版：一行一列、所有子项都放进这一格，子项自己的网格位置不起作用。

`Stack::overlay_layer()` 是一个铺满已经定位的父级的叠层：绝对定位，溢出裁剪，指针事件关掉。舞台上的 HUD 和弹幕用它当容器，节点池仍由你挂。Rust 预设默认不参与命中。空白处的按下也要落在这个容器上时，接 `.hittable()`。

## 布局字段

`Stack::from_layout` 承接已经解析好的 `LayoutStyle`，默认可点。`.with_layout` 就地改布局字段。

`.gap`、`.align`、`.justify`、`.width`、`.height`、`.grow`、`.shrink`、`.padding` 改预设上的对应项。`.surface` 写语义背景。`.outline` 要同时给出边框色和宽度，缺一则不画边框。

## 自绘

`.painter` 可以自绘这个容器的外观。要在内建绘制上加装饰，在 `paint` 里调用 `cx.draw_default()`。

容器的渐变 `mask-image`（`layout.paint.mask`）也作用在自绘内容上，和作用在背景上一样。一条阴影加一个 mask 就能画出只朝一侧的光晕；伸出容器的那部分取盒边上的 mask 值。子节点不受影响。细节见 [布局](../reference/layout.md) 的绘制一节。

## 命中

二维码卡片的空白也要接到指针时，用 `Stack::column(...).hittable()`，事件才归这张容器。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `Stack::row(gap)` | — | 水平排列，宽度随内容，子项垂直居中 |
| `Stack::fill_row(gap)` | — | 水平占满父级剩余宽度，子项可以收缩 |
| `Stack::column(gap)` | — | 竖直排列，高度随内容，宽度占满父级。视图函数 `column()` 是 `widget(Stack::column(0.0))` |
| `Stack::fill_column(gap)` | — | 竖直占满剩余高度 |
| `Stack::bar(gap)` | — | 水平占满整行，自己不伸展 |
| `Stack::spacer()` | — | 零宽 flex-grow。基准宽度是 0，否则会把兄弟挤出去 |
| `Stack::layers()` | — | 叠层：子项共用同一块区域，各自对齐，容器随最大的子项。CSS 写作 `display: layers` |
| `Stack::overlay_layer()` | — | 铺满已经定位的父级的叠层：绝对定位，溢出裁剪，指针事件关掉。Rust 预设默认不参与命中 |
| `Stack::from_layout` | `LayoutStyle` | 已经解析好的布局。默认可点 |
| `.with_layout` | — | 就地改布局字段 |
| `.gap` | — | 改预设上的对应项。间距不为 0 时直接构造 |
| `.align` | — | 改预设上的对应项 |
| `.justify` | — | 改预设上的对应项。叠层没有主轴，这里改子项的水平对齐 |
| `.width` | — | 改预设上的对应项 |
| `.height` | — | 改预设上的对应项 |
| `.grow` | — | 改预设上的对应项 |
| `.shrink` | — | 改预设上的对应项 |
| `.padding` | — | 改预设上的对应项 |
| `.surface` | — | 语义背景 |
| `.outline` | — | 边框色和宽度都要给，缺一则不画边框 |
| `.hittable` | — | 空白处的按下落在这个容器上 |
| `.painter` | — | 自绘外观。装饰时在 `paint` 里调用 `cx.draw_default()` |

## 事件

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| 无 | 无 | 无 |

## 插槽

| 插槽 | 说明 |
| --- | --- |
| 子节点 | 排成一行、一列，或铺成一层叠层。函数侧是 `.children`。叠层上的节点池仍由你挂 |

## 参见

[总览](index.md) · [控件](../reference/components.md)
