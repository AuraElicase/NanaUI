# Progress

`progress(max)` 创建一条进度：确定的，或者不知道还要多久时，不确定的。`max` 是 `f64`，在构造时给定。函数注释要你再绑 `.value(…)`。

模板里的标签是 `<Progress>`。

## 基本用法

:::api

```rust view
use nana_ui::runtime::view;

view! {
    <Progress max=100 value={done} label="导出" />
}
```

```rust rust
use nana_ui::runtime::view::progress;

progress(100.0).value(done).label("导出")
```

:::

模板里的 `max=100` 展开成 `100_f64`。手写时参数是 `f64`，写成 `100.0`。`value` 可以是常量、`Signal<f64>` 或闭包。

## 取值

显示的值夹在 `0` 和 `max` 之间。非有限的 `value` 按 `0`。`max` 不是有限正数时，组件把它收成一个极小的正数，不拿它做除零。无障碍同时给出百分比、最小、最大和当前数值。

无障碍的值是四舍五入后的百分比，同时带上最小、最大和当前数值。填充比例是夹紧后的 `value / max`。

`value` 可以跟着信号变。确定的进度只显示你写进去的数，自己不加动画相位。

## 不确定进度

不知道还要多久时，写 `indeterminate`。轨道上不再有代表数值的填充，而是一段约占轨道 35% 的填充从起点进来、从终点出去，一遍接一遍。

:::api

```rust view
use nana_ui::runtime::view;

view! {
    <Progress max=1 indeterminate label="正在连接" />
}
```

```rust rust
use nana_ui::runtime::view::progress;

progress(1.0).indeterminate(true).label("正在连接")
```

:::

这一扫是组件自己起的循环时间线，宿主不用推。一遍的时长读安装主题的骨架屏脉动（`MotionRole::SkeletonPulse`，默认 1.4 秒）。隐藏、改回确定、或卸下时停下，重新挂上时接着扫。

减弱动态效果打开时不扫：整条轨道铺一层很淡的填充色（填充色的 35%），一眼看去不会当成「已完成」。偏好切换的那一刻就开始或停下，不等下一次数据变化。

无障碍上不报数值，也没有最小、最大，只标成 busy，和不带 `value` 的 `<progress>` 一样。`indeterminate` 改回 `false`，百分比和数值回来。

## 标签和轨道

有标签时，标签在轨道上方。标签可以不给。没有标签、也不可取消时，高度就是轨道本身。进度不自己跑表，数值由应用写。

轨道默认厚 6，是 Subtle 底；填充走主题的 Indicator 配方，默认是 Accent。三者都能换：`thickness` 给厚度（不是正数时仍按 6），`track` 和 `fill` 给语义色。圆角始终是厚度的一半。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::theme::SemanticColorRole;

view! {
    <Progress max=1 value={ratio} thickness={3.0} track={SemanticColorRole::BorderSoft} fill={SemanticColorRole::Accent} />
}
```

```rust rust
use nana_ui::runtime::view::progress;
use nana_ui::theme::SemanticColorRole;

progress(1.0)
    .value(ratio)
    .thickness(3.0)
    .track(SemanticColorRole::BorderSoft)
    .fill(SemanticColorRole::Accent)
```

:::

`label` 的类型是 `Option<Arc<str>>`。不写就是没有标题。有标题时字重走 medium。

## 可取消

可取消不是字段。组件 `Progress::cancellable(true)` 才接收指针、才可聚焦。激活时如果 `cancellable` 仍为真，发 `ProgressCancelled`，进度本身不删、不停表。听这个事件用 `.on(|_: &ProgressCancelled| …)`。`<Progress>` 没有对应的 `on_` 方法。进度控件不拥有计时器。

## 上限

`max` 只是构造参数，不是之后能绑的字段。视图函数没有 `.max`。天花板要变，用 `.bind(|progress| progress.max = next)`，或重新构造。

## 属性

可绑的字段是 `value`、`label`、`indeterminate`、`thickness`、`track` 和 `fill`。没有 `model`。

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `value` | `f64` | 可以是常量、`Signal<f64>` 或闭包。显示夹在 `0` 和 `max` 之间。非有限按 `0`。可以跟着信号变 |
| `label` | `Option<Arc<str>>` | 可以不给。有标签时在轨道上方，字重走 medium。不写就是没有标题 |
| `max` | `f64` | 构造时给定，不是之后能绑的字段。视图函数没有 `.max`。不是有限正数时收成一个极小的正数 |
| `indeterminate` | `bool` | 不知道还要多久：一段填充沿轨道扫，减弱动态效果时改成整条淡填充；无障碍只标 busy，不报数值 |
| `thickness` | `Option<f32>` | 轨道厚度。不写或不是正数时是 6 |
| `track` | `Option<SemanticColorRole>` | 轨道颜色。不写是 Subtle |
| `fill` | `Option<SemanticColorRole>` | 填充颜色。不写走主题的 Indicator 配方 |

## 事件

没有事件，也没有 `model`。激活时如果 `cancellable` 仍为真，发 `ProgressCancelled`。

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| `.on` | `&ProgressCancelled` | 可取消不是字段。`Progress::cancellable(true)` 才接收指针、才可聚焦。进度本身不删、不停表。`<Progress>` 没有对应的 `on_` 方法。进度控件不拥有计时器 |

## 插槽

| 插槽 | 说明 |
| --- | --- |
| 无 | 无 |

## 参见

[总览](index.md) · [控件](../reference/components.md)
