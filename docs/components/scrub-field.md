# ScrubField

`ScrubField` 是专业工具属性面板里那种「在数字上左右拖动改值」的字段。DCC、CAD 和音频软件的属性面板都用这种字段。它没有模板标签，用 `widget(ScrubField::new(值))` 放进视图。

## 基本用法

```rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{ScrubChanged, ScrubField, ScrubInput};

widget(
    ScrubField::new(1.0)
        .range(0.0, 10.0)
        .step(0.1)
        .precision(2)
        .label("X")
        .unit("px"),
)
.on(|e: &ScrubInput| { /* 拖动中的预览 */ })
.on(|e: &ScrubChanged| { /* 提交：写进撤销历史 */ })
```

字段左边画 `label`，右边画「数值 单位」。

## 拖动

- 按下后左右拖动来改值。每拖 1 px，值变化 `drag_rate`；没有设置时，每像素变化一个 `step`。
- 按住 Ctrl（macOS 上是 Cmd）时变化量乘 0.1，按住 Shift 时乘 10。拖动途中可以随时切换。
- 值按 `precision` 取整，并夹在 `range` 的上下限之内。
- 指针移动不足 2 px 算单击，不算拖动。

拖动中每次取值变化发一次 `ScrubInput { value }`，松手时发一次 `ScrubChanged { value }`。应用据此只把提交写进撤销历史。拖动的起止另外发 `ScrubDragging { dragging }`。被取消的按下（指针被系统收走）会把值回退到拖动之前，再发一次 `ScrubInput`，不发 `ScrubChanged`。

字段把滚轮让给它所在的面板，自己不处理滚轮。

## 键入

不拖动直接松开、按 Enter 或 Space，字段就进入键入状态：

- 组装好的 `NumberInput` 显示当前值，全选并获得焦点。
- Enter 或焦点离开时提交，发 `ScrubChanged`。
- Escape 还原，不发事件。

键入结束后，焦点回到字段。`editing` 表示字段是否处于键入状态。

## 键盘步进

焦点在字段上时：

| 按键 | 效果 |
| --- | --- |
| ↑ / → | 加一个 `step` |
| ↓ / ← | 减一个 `step` |
| PageUp / PageDown | 加 / 减十个 `step` |
| Home / End | 跳到下限 / 上限（只在设置了上下限时） |

每一步都是一次提交。读屏的 `Increment` / `Decrement` 也走这里。

## 混合值

多选时如果各对象的值不一致，设 `.mixed(true)`，或者调用 `set_scrub_field_value(字段, 值, true)`。混合时字段显示「—」，读屏没有数值。之后一次拖动、步进或键入会把所有对象设成同一个新值，并清掉混合状态。拖动从 `value` 开始，应用通常把它设成活动对象的值。

## 应用写值

`set_scrub_field_value(字段, 值, 混合)` 写入应用侧的值，不发事件。

手工用 `create_component` 创建时，要自己调用 `assemble_scrub_field` 来组装标签、数值文字和输入框。放进视图时由视图负责组装。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `ScrubField::new(value)` | `f64` | 初始值 |
| `.range(min, max)` | `f64, f64` | 上下限，拖动、键入和步进都会夹在其中 |
| `.step(step)` | `f64` | 方向键的步长；未设置 `drag_rate` 时也是每像素的变化量 |
| `.precision(digits)` | `u8` | 显示和取整的小数位数 |
| `.drag_rate(per_pixel)` | `f64` | 每拖 1 px 的变化量 |
| `.label(text)` | 文字 | 值前面的标签，同时是读屏名称 |
| `.unit(text)` | 文字 | 值后面的单位 |
| `.mixed(bool)` | `bool` | 显示混合值 |
| `.size(size)` | `ControlSize` | 控件高度 |
| `.disabled(bool)` | `bool` | 禁用 |

## 事件

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| `ScrubInput` | `value: f64` | 拖动中每次取值变化；取消时回退到拖动之前的值 |
| `ScrubChanged` | `value: f64` | 提交：松手、键入后的 Enter 或失焦、键盘步进 |
| `ScrubDragging` | `dragging: bool` | 拖动开始和结束 |

## 插槽

| 插槽 | 说明 |
| --- | --- |
| 无 | 标签、数值文字和输入框由字段自己组装 |

## 参见

[NumberInput](number-input.md) · [RangeField](range-field.md) · [总览](index.md)
