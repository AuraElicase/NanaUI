# InteractionRequestCard

`InteractionRequestCard` 是一个中性的请求与审批卡片表面：标题来自 `title`，提示与正文通过 `prompt` 和 `body` 槽位传入，键值表单项通过 `fields` 描述，底部操作按钮通过 `actions` 传入。它只负责组合与保留这棵子树，不保存业务状态，也不替应用发出工作流或权限事件；宿主直接监听自己挂进去的输入控件和按钮。

控件表里没有 `<InteractionRequestCard>` 模板标签。在声明式视图里用 `widget(InteractionRequestCard::new(...))` 配合 `.child_slot` / `.children_slot` 挂载，或用命令式 API 组装。

## 基本用法

每个 `InteractionRequestField` 具有稳定的 `key`（非空且不含 `/`）和宿主创建的控件节点 ID。组件会自动为每个字段创建并按 `key` 保留对应的 `FormField` 外壳：更新同一个 `key` 的标签、提示或错误文案时，外壳和内部控件的身份保持不变，输入焦点与编辑历史不会因为相邻字段变化而丢失。

```rust
use nana_ui::runtime::{
    Button, FormField, InteractionRequestCard, InteractionRequestField, Text, TextInput,
};

let input = cx.create_detached_component(document, TextInput::new(""))?;
let prompt = cx.create_detached_component(document, Text::new("请补充连接信息"))?;
let action = cx.create_detached_component(document, Button::new("继续"))?;

let card = cx.create_component(
    document,
    InteractionRequestCard::new("需要输入")
        .prompt(prompt.stable_id())
        .actions([action.stable_id()])
        .field(
            InteractionRequestField::new("host", "主机地址", input.stable_id())
                .hint("例如 127.0.0.1:8080"),
        ),
)?;
cx.assemble_interaction_request_card(card)?;
```

字段的值、校验和动作由应用写回自己持有的 `TextInput`、`Select` 或 `Button`；卡片在无障碍树中投影为以 `title` 命名的 `Region`（`loading` 时带 `busy = true`）。

## 属性

| 属性 / 方法 | 类型 | 说明 |
| --- | --- | --- |
| `InteractionRequestCard::new(title)` | `impl Into<Arc<str>>` | 卡片标题，同时作为 `Region` 的无障碍名称 |
| `.kind(kind)` | `CardKind` | 卡片表面样式，默认 `CardKind::Surface` |
| `.loading(bool)` | `bool` | 加载态（无障碍 `busy` 标志），默认 `false` |
| `.prompt(node)` | `StableNodeId` | 提示节点槽位 |
| `.body(node)` | `StableNodeId` | 正文节点槽位 |
| `.actions(nodes)` | `impl IntoIterator<Item = StableNodeId>` | 操作按钮节点列表 |
| `.field(field)` / `.fields(fields)` | `InteractionRequestField` | 单个或整批键值字段描述 |
| `.style(style)` | `NodeStyle` | 根节点样式 |
| `field_wrappers()` | `Iterator<Item = (&str, StableNodeId)>` | 组装后查询每个字段 `key` 对应的 `FormField` 包装节点 ID |

`InteractionRequestField` 的构造方法：

| 方法 | 类型 | 说明 |
| --- | --- | --- |
| `InteractionRequestField::new(key, label, control)` | `(Arc<str>, Arc<str>, StableNodeId)` | 字段稳定键、标签与宿主创建的控件节点 |
| `.hint(text)` | `impl Into<Arc<str>>` | 传给外层 `FormField` 的辅助提示文字 |
| `.error(text)` | `impl Into<Arc<str>>` | 传给外层 `FormField` 的校验错误文字 |

## 事件

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| 无 | 无 | 卡片自身不发业务事件；请直接监听传入 `fields` 和 `actions` 的各控件事件 |

## 插槽与子节点顺序

组装后子节点按固定顺序排列：

1. 各 `fields` 对应的 `FormField` 包装节点（内部挂载宿主传入的 `control`）；
2. `prompt`（若提供）；
3. `body`（若提供）；
4. `actions` 中的各操作节点。

被移除的旧子节点会自动停放（park），不会残留幽灵节点。

## 参见

[Card](card.md) · [FormField](form-field.md) · [DynamicForm](dynamic-form.md) · [总览](index.md)
