# TreeView

`TreeView` 把你给出的稳定节点、展开事实和层级画成 disclosure 行。它不读路径，也不监视文件系统。折叠起来的子节点不投影。

## 基本用法

`TreeView::new(nodes)` 接收 `TreeNode<Arc<str>>`。叶子用 `TreeNode::leaf(id, label)`，分支用 `TreeNode::branch(id, label, expanded, children)`。`.icon`、`.selected`、`.disabled` 写在节点上。

树的 `.size` 默认小号。`.style` 整份替换样式，要放在会改布局的构造之前。控件表里没有 `<TreeView>`。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::{TreeNode, TreeView};

view! {
    <Widget
        of={TreeView::new([
            TreeNode::branch(
                "src".into(),
                "src",
                true,
                [TreeNode::leaf("main".into(), "main.rs").selected(true)],
            ),
            TreeNode::leaf("cargo".into(), "Cargo.toml"),
        ])}
    />
}
```

```rust rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{TreeNode, TreeView};

widget(TreeView::new([
    TreeNode::branch(
        "src".into(),
        "src",
        true,
        [TreeNode::leaf("main".into(), "main.rs").selected(true)],
    ),
    TreeNode::leaf("cargo".into(), "Cargo.toml"),
]))
```

:::

默认宽度是填满，高度按可见行数和内边距算出。你自己写了高度时，这个高度会留下，方便放进固定视口。选中项的 id 用 `selected_id` 读。禁用节点不会被选中。

## 展开和选中

`apply_event` 由树自己改展开和选中：`Toggle` 只翻转分支，`Select` 选中那个还没禁用的节点，并清掉其他选中。

`navigate` 按 `TreeNavigation` 在可见行上移动，然后应用得到的事件。可见行从 `visible_rows` 读，深度记在 `TreeRowData` 里。

## 多选

`.multiple(true)` 让树维护一个选中集合，以及集合里的一个活动项：

- 按下一行：只选它。
- Ctrl（macOS 上是 Cmd）+ 按下：把这一行加入或移出选中集合。
- Shift + 按下：选中从锚点到这一行之间的所有可见行。
- 焦点在树上时，Shift + 方向键 / Home / End 移动活动项并扩展范围。

每次选中集合变化都发 `TreeSelectionChanged { selected, active }`，`selected` 按树的顺序排列。单选时仍然发 `TreeViewEvent::Select`。`selected_ids` 读选中集合，`active_id` 读活动项。

## 开关列

`.columns([TreeToggleColumn::new(图标, 名称)])` 在每一行末尾画一列图标开关，例如可见、锁定。行的开关状态是 `TreeNode::toggles`：

- 开着的图标用次要文字色，关着的用更淡的颜色。
- 按下开关会翻转它，不改变选中。
- 按住后沿着这一列拖过其他行，会把每一行都设成同一个值。

每个变化发一次 `TreeToggleChanged { id, column, on }`。树自己更新节点上的状态；应用如果不接受，就把节点写回去。

## 拖放

`.reorderable(true)` 后可以把行拖到另一行上，落点分三种：

- 指针在目标行的上四分之一：`Before`，在行的上缘画一条线。
- 指针在下四分之一：`After`，在行的下缘画一条线。
- 指针在中间：`Inside`，给整行画框。

被拖的是选中集合（从选中的行开始拖时），否则只是这一行。目标是被拖行自己或它的子孙时不显示落点。松手发 `TreeDropRequested { rows, intent }`，树本身不移动任何节点，由应用决定接不接受。不拖动、直接松开一个已选中的行，会只选中这一行。

## 行内重命名

`.renamable(true)` 后，双击一行或在焦点所在的树上按 F2，就会在标签位置打开一个输入框，选中原来的名字：

- Enter 或焦点离开树：发 `TreeRenameRequested { id, text }`，焦点回到树。
- Escape：取消，不发事件。

树不改节点的名字，由应用决定接不接受。`renaming` 读正在重命名的行。手工调用是 `begin_tree_rename` / `end_tree_rename`。

## 从列表拖入

从 `ReorderList` 拖到树上时，回传的是稳定节点，以及 `before`、`inside` 或 `after`。树不推断这是目录还是文件。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `nodes` | `TreeNode<Arc<str>>` | `TreeView::new(nodes)` 接收的稳定节点 |
| `TreeNode::leaf(id, label)` | — | 叶子 |
| `TreeNode::branch(id, label, expanded, children)` | — | 分支。`expanded` 是展开事实 |
| `.icon` | — | 写在节点上 |
| `.selected` | — | 写在节点上 |
| `.disabled` | — | 写在节点上。禁用节点不会被选中 |
| `.size` | — | 默认小号 |
| `.multiple(bool)` | `bool` | 多选：Ctrl 切换、Shift 范围、活动项 |
| `.columns(列)` | `TreeToggleColumn` | 行尾的开关列；状态写在 `TreeNode::toggles` |
| `.reorderable(bool)` | `bool` | 行可以拖到别的行之前、之内或之后 |
| `.renamable(bool)` | `bool` | F2 或双击时行内重命名 |
| `.style` | — | 整份替换样式，要放在会改布局的构造之前 |

## 事件

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| `TreeViewEvent::Toggle` | `id` | `apply_event` 只翻转分支 |
| `TreeViewEvent::Select` | `id` | 选中那个还没禁用的节点，并清掉其他选中 |
| `TreeSelectionChanged` | `selected`, `active` | 选中集合或活动项变了 |
| `TreeToggleChanged` | `id`, `column`, `on` | 一行的开关被设成 `on` |
| `TreeDropRequested` | `rows`, `intent` | 拖放请求；`intent` 是目标与 `Before` / `Inside` / `After` |
| `TreeRenameRequested` | `id`, `text` | 重命名提交 |

## 插槽

没有具名插槽。节点来自 `TreeNode` 数据。

## 参见

[总览](index.md) · [控件](../reference/components.md)
