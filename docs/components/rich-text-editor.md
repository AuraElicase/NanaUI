# RichTextEditor

`RichTextEditor` 就地编辑一份 `RichText`：所见即所得。它和 [RichTextView](rich-text-view.md) 是同一种文本节点，走同一条 `SetRichText` 路径、同一份保留排版。同一份文档、同样宽度，两者断出的行完全一样。不是两份排版碰巧一致，而是本来就是同一份。

文档归应用：每次改动发出 `RichTextEditorEvent::Changed(value)`，应用存下这个值，下次重建时原样交回。交回相等的值什么都不提交。控件只持有应用不该管的编辑状态：选区、输入法的预编辑、下一个字用的样式、撤销历史。

控件表里没有 `<RichTextEditor>`。标签 `rich-text-editor` 只造出纯文本的初始值。

## 基本用法

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::RichTextEditor;
use nana_ui::runtime::rich::RichText;

view! {
    <Widget of={RichTextEditor::new(RichText::new("说点什么")).font_size(20.0)} />
}
```

```rust rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::RichTextEditor;
use nana_ui::runtime::rich::RichText;

widget(RichTextEditor::new(RichText::new("说点什么")).font_size(20.0))
```

:::

编辑器的字号、字体族和宽度设成和展示框一样，两边就按同样的方式排版。

## 工具栏

工具栏不直接改文档，而是向编辑器发命令：`cx.rich_edit(entity, command)`。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::{RichEditCommand, RichTextEditor};
use nana_ui::runtime::rich::{RichObject, RichSpanStyle, RichText};

let editor = cx.create_component(document, RichTextEditor::new(RichText::new("")))?;
cx.rich_edit(editor, RichEditCommand::SetAttrs(RichSpanStyle::new().bold()))?;
cx.rich_edit(editor, RichEditCommand::InsertObject(RichObject::chip(7, "等待 500ms", 1)))?;

view! {
    <Widget of={RichTextEditor::new(RichText::new(""))} />
}
```

```rust rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{RichEditCommand, RichTextEditor};
use nana_ui::runtime::rich::{RichObject, RichSpanStyle, RichText};

let editor = cx.create_component(document, RichTextEditor::new(RichText::new("")))?;
cx.rich_edit(editor, RichEditCommand::SetAttrs(RichSpanStyle::new().bold()))?;
cx.rich_edit(editor, RichEditCommand::InsertObject(RichObject::chip(7, "等待 500ms", 1)))?;

widget(RichTextEditor::new(RichText::new("")))
```

:::

| `RichEditCommand` | 做什么 |
| --- | --- |
| `SetAttrs(style)` | 把 `style` 逐字段盖到选区上。没有选区时，它成为下一个输入字符的样式 |
| `ClearFormatting` | 去掉选区上所有 span；没有选区时，下一个字不带样式 |
| `InsertText(text)` | 用 `text` 替换选区，样式和打字一样 |
| `InsertObject(object)` | 用一个内联对象替换选区：贴纸、图片，或零宽的标记 chip |
| `SetObject { offset, object }` | 换掉 `offset` 处的对象 |
| `SetRuby(Some(text))` / `SetRuby(None)` | 给选区加注音（替换它碰到的注音）/ 删掉选区碰到的注音。只做横排 |
| `Select(range)` / `SelectAll` | 改选区（按字符边界对齐） |
| `Undo` / `Redo` | 撤销、重做 |

工具栏的按下态读 `SelectionChanged { selection, attrs }`。`attrs` 是三态的：`attrs.style` 里有值、`attrs.mixed` 里没有对应位，表示整个选区都是这个值；没值也不 mixed，表示整个选区都没设；`mixed` 有这一位，表示选区里不一致。`RichAttrMask` 每个字段一位，字宽是 `STRETCH`，背景色是 `BACKGROUND_COLOR`；`ClearAttrs(mask)` 只去掉这些字段。

## 输入

- 打字继承光标前那个字的样式；`SetAttrs` 给光标的样式优先。连续打字合成一步撤销。`typing_style_policy(RichTypingStylePolicy::Plain)` 让打字、预编辑和纯文本粘贴不再继承相邻的字，只用节点样式；`SetAttrs` 给光标的样式照样生效，工具栏在光标处显示的也是将要输入的样式。默认 `Inherit`。富文本粘贴在两种策略下都保留复制内容自带的样式。
- 输入法的预编辑按将要提交的样式画出来，加下划线。它不进文档，提交时才进。候选框锚在编辑器的光标上，空文档也一样。
- 聚焦时光标闪烁（显示、隐藏各 500 ms）；文档、预编辑或光标位置每次变化都立即显示并重新计时（包括光标没动的向后删除），与 `TextInput` 一致；窗口失焦时常亮。
- 方向键按字素移动，Ctrl 按词；上下按行；Home / End 到行首行尾，加 Ctrl 到文档首尾；Shift 扩展选区。
- Ctrl+C / X 把纯文本写进系统剪贴板，同时在进程内记住最近复制的这一段富文本和写入后剪贴板的变更序号。粘贴时剪贴板仍是这次写入（序号相同、文字相同）才带着样式和对象贴回去；别的程序或别的输入框写进同样的文字，按纯文本贴。宿主没有变更序号时（Linux）退回到按文字匹配。剪贴板里没有文字时发出 `PasteRequested`，应用可以从自己的剪贴板格式插一张图。
- 编辑菜单和右键菜单用 `cx.rich_clipboard(entity, RichClipboardCommand, services)`：`Copy`、`Cut`、`Paste` 与快捷键相同，`PastePlainText` 即使剪贴板里是自己的复制也按纯文本贴。只读编辑器只能 `Copy`；预编辑期间都不执行；没有选区的复制不改剪贴板，写入失败时剪切不删文字。
- Enter 插入换行。要 Enter 发送，在应用的按键策略里先截住它（它比编辑器先看到按键）。

## 内联对象

贴纸和图片与文字一起换行。标记 chip（`RichObject::chip`）宽度为 0，只在编辑器里画成一道细标记，展示框不画，所以两边断行一致。`RichObject::label(id, "@名字", RichLabelStyle { fill, text, scale, icon, .. })` 是应用自己配色的标签（例如带头像的人名），编辑器和展示框画得一样、占一样的宽度，选区和删除都把它当一个字。点到对象发出 `ObjectActivated { id, offset }`；一次编辑删掉对象，发出 `ObjectRemoved { id, offset }`。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `value` | `RichText` | 文档。每次改动由 `Changed` 报出 |
| `read_only` | `bool` | 可以选择、复制，不能改 |
| `disabled` | `bool` | 不能聚焦 |
| `typing_style_policy` | `RichTypingStylePolicy` | 打字是否继承相邻文字的样式，默认 `Inherit` |
| `font_size` / `font_family` / `line_height` / `width` | | 同 [RichTextView](rich-text-view.md) |

## 事件

| `RichTextEditorEvent` | 时机 |
| --- | --- |
| `Changed(RichText)` | 文档变了 |
| `SelectionChanged { selection, attrs }` | 选区或选区处的样式变了 |
| `ObjectActivated { id, offset }` | 点到一个内联对象 |
| `ObjectRemoved { id, offset }` | 编辑删掉了一个内联对象 |
| `PasteRequested` | 粘贴时剪贴板里没有文字 |

## 参见

[RichTextView](rich-text-view.md) · [总览](index.md) · [文本引擎](../reference/text-engine.md#富文本编辑器)
