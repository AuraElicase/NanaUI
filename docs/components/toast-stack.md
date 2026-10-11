# ToastStack

窗口一角的一列通知：最新的一条贴在锚定的底边，旧的往上排。`ToastStack::new()` 创建它，提示用 `AppContext::push_toast` 放进去、`remove_toast` 拿出来。控件表里没有 `<ToastStack>` 标签，两种写法都调用 `widget(ToastStack::new())`。

## 基本用法

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::ToastStack;

view! {
    <Widget of={ToastStack::new().max_visible(3).label("通知")} />
}
```

```rust rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::ToastStack;

widget(ToastStack::new().max_visible(3).label("通知"))
```

:::

放提示、听事件、拿掉，都在应用手里：

```rust
use nana_ui::runtime::{PopoverAlignment, Toast, ToastDismissed, ToastProgress, ToastStack, ToastTone};

let mut corner = ToastStack::new();
corner.place_in(window, PopoverAlignment::End, 360.0, insets, 420.0);
let stack = cx.create_component(document, corner)?;

let toast = cx.push_toast(
    stack,
    Toast::new("正在优化模型", ToastTone::Info)
        .progress(ToastProgress::Indeterminate)
        .dismissible(true),
)?;
cx.on(toast, |_, _: &ToastDismissed, _| { /* 让应用调用 remove_toast */ })?;

// 任务有了进度：原地改这一条
cx.update_component(toast, |toast, _| {
    toast.progress = Some(ToastProgress::Determinate(0.6));
})?;
cx.remove_toast(stack, toast)?;
```

`push_toast` 先装配提示（[`assemble_toast`](toast.md#复合形式)），再把它作为最新的一条放进栈，返回提示自己的实体：事件听它，更新也写它。它从不挪焦点。

## 放置

`ToastStack::place_in(viewport, align, max_width, insets, max_height)` 与 `Toast::place_in`、`Panel::viewport` 同一个预留合同：把栈钉进 `insets` 留出的空闲区域，`align` 决定栈在空闲宽度里的位置，也决定提示在栈里的对齐（`End` 靠右），`max_width` 封顶宽度，`max_height` 封顶提示可占的高度，栈贴住预留的底边往上长。`viewport` 是定位宿主自己的盒子。它只写位置和尺寸。窗口大小或预留区变了，再调一次；栈会按新的高度重新排。

## 先量后显、排队

刚放进来的提示先在栈里脱流、不可见地布局一次：不命中、读屏听不到、计时器不走，栈上已有的提示不动。那一趟布局量出它的高度后，栈决定显示哪些：从最新的往旧的数，少于 `max_visible` 条且高度加间距（配方的 `stack_gap`）不超过 `max_height` 的显示。最新的一条即使单独超高也显示。其余的排队：不布局、读屏听不到、计时器暂停。有空位时（拿掉一条、栈放得更高），排队的重新量过再回来。

`shown_toasts(stack)` 按最新优先列出在屏上的提示，不含排队的、还在量的和正在离开的。

## 进场、离开和收拢

动效读配方的 `motion`：进场从透明、从 `enter_offset_y` 和 `enter_scale` 回到原位，时长和缓动是 `enter`；`remove_toast` 让在屏上的那条在原位把进场倒着放一遍（`exit`），播完才拿掉，其余的按 `stack_move` 滑到新的位置。排队的或还在量的直接拿掉。`clear_toasts` 把所有提示照这样拿掉。

正在进场时被挤动的提示从它站着的地方滑过去，最后停在原位、没有变换。减弱动态效果时什么都不播：直接出现，直接消失，其余的直接到位。

## 计时

栈里的提示只在显示时计时。指针在栈里任何一条提示上，或焦点在栈里时，栈里所有的计时器暂停；离开后从下一帧起按各自剩余的时间继续。到时在那一条提示上发 `ToastDismissed`，拿不拿掉由应用决定。没有动效在播时，栈只为最早到期的那个计时器唤醒宿主。

## 指针和模态

栈本身不接收指针，只有提示接收：提示之间的空隙和上方的空白落到下面的舞台。

模态对话框的卡片画在 `z_index` 1000。应用把栈放在模态层之上时（同一层叠上下文里 `z_index` 高于 1000，或在模态宿主之后的兄弟上更高），模态打开期间按在栈里提示上的指针到达提示，动作和关闭照常工作，不算点在模态外面，模态不关；焦点留在模态里，Tab 仍只在模态的控件间循环。栈放在模态之下时照常被模态盖住。栈不要挂在 `OverlayHost` 下当浮层激活，按普通的定位节点放。

## 无障碍

栈是一个 Region，名字是 `label`，不给时是框架文案 `toast.region`（「通知」）。在屏上的每一条提示是一个 live 的 Status（见 [Toast](toast.md#无障碍)），排队的、还在量的和正在离开的都听不到。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `max_visible` | `usize` | 同时在屏上的最多条数，正在离开的不算。默认 3 |
| `label` | `Option<Arc<str>>` | Region 的名字，不给时是 `toast.region` |
| `style` | `NodeStyle` | `place_in` 写其中的位置、宽度、最大高度和对齐；`z_index` 由应用给 |

## 方法

| 方法 | 说明 |
| --- | --- |
| `AppContext::push_toast(stack, toast)` | 装配并放进最新的一条，返回提示的实体。先量后显，不挪焦点 |
| `AppContext::remove_toast(stack, toast)` | 在屏上的播完退场再拿掉，其余收拢；排队的和还在量的直接拿掉。栈里没有这条时返回 `false` |
| `AppContext::clear_toasts(stack)` | 全部拿掉 |
| `AppContext::shown_toasts(stack)` | 在屏上的提示，最新优先 |
| `ToastStack::place_in(..)` | 见[放置](#放置) |

## 事件

栈自己不发事件。`ToastDismissed`、`ToastAction` 发在各条提示上，见 [Toast](toast.md#事件)。

## 插槽

| 插槽 | 说明 |
| --- | --- |
| 无 | 提示用 `push_toast` 放进去，不当作子节点挂 |

## 参见

[总览](index.md) · [Toast](toast.md) · [控件](../reference/components.md)
