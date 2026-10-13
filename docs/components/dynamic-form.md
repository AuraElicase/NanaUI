# DynamicForm

`DynamicForm` 用稳定的字段描述（`DynamicFormField`）组装设置页、扩展配置和请求表单。每个字段的 `id` 是应用拥有的稳定键（非空且不含 `/`）：同一个键更新时保留底层控件、输入焦点和编辑撤销历史；增删邻近字段不会重建未变字段。

控件表里没有 `<DynamicForm>` 模板标签，两种写法都用 `widget(DynamicForm::new(...))` 挂载。要做专业工具检查器那种密集的两列属性面板（可拖拽公共标签列宽、支持混合值），请用 [PropertyGrid](property-grid.md)。

## 基本用法

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DynamicForm, DynamicFormEvent, DynamicFormField, DynamicFormOption};

widget(DynamicForm::new([
    DynamicFormField::section("general", "常规"),
    DynamicFormField::field("name", "项目名称", "NanaApp")
        .binding_identity("project-name")
        .hint("显示在标题栏"),
    DynamicFormField::toggle("enabled", "启用同步", true),
    DynamicFormField::choice(
        "mode",
        "运行模式",
        "safe",
        [
            DynamicFormOption::new("safe", "安全"),
            DynamicFormOption::new("fast", "快速"),
        ],
    ),
    DynamicFormField::action("reset", "恢复默认").danger(true),
]))
.on(|event: &DynamicFormEvent| {
    // 按字段 id 更新应用状态与校验结果
})
```

```rust rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DynamicForm, DynamicFormEvent, DynamicFormField, DynamicFormOption};

widget(DynamicForm::new([
    DynamicFormField::section("general", "常规"),
    DynamicFormField::field("name", "项目名称", "NanaApp")
        .binding_identity("project-name")
        .hint("显示在标题栏"),
    DynamicFormField::toggle("enabled", "启用同步", true),
    DynamicFormField::choice(
        "mode",
        "运行模式",
        "safe",
        [
            DynamicFormOption::new("safe", "安全"),
            DynamicFormOption::new("fast", "快速"),
        ],
    ),
    DynamicFormField::action("reset", "恢复默认").danger(true),
]))
.on(|event: &DynamicFormEvent| {
    // 按字段 id 更新应用状态与校验结果
})
```

:::

手工通过 `create_component` 创建时，需调用 `cx.assemble_dynamic_form(form)?`；在声明式视图中由框架自动组装。

## 字段类型与编辑历史

`DynamicFormField` 提供七种行描述：

| 构造器 | 生成的控件 | 可选修饰方法 |
| --- | --- | --- |
| `DynamicFormField::section(id, label)` | `SettingsCard` 分组卡片（后续字段归入该组，直到下一个 `section`） | — |
| `DynamicFormField::toggle(id, label, checked)` | `Switch` | `.disabled(bool)` |
| `DynamicFormField::choice(id, label, selected, options)` | `SettingsRow` + 单选 `Dropdown` | `.disabled(bool)`、`.hint(text)`；选项用 `DynamicFormOption::new(value, label).disabled(bool)` |
| `DynamicFormField::field(id, label, value)` | `SettingsRow` + `TextInput`（或 `.multiline(true)` 时为 `TextArea`） | `.multiline(bool)`、`.secure(bool)`、`.disabled(bool)`、`.hint(text)`、`.binding_identity(id)` |
| `DynamicFormField::text(id, value)` | 静态 `Text` | — |
| `DynamicFormField::error(id, value)` | 危险态 `ValidationMessage` | — |
| `DynamicFormField::action(id, label)` | `Button`（默认 `Subtle`，`.danger(true)` 时为 `Danger`） | `.enabled(bool)`、`.danger(bool)` |

**编辑身份（`binding_identity`）**：当同一个字段 `id` 切换到另一个业务对象（例如列表选中了另一条记录）时，给 `.binding_identity(...)` 传入新对象的唯一标识，表单会自动清空该输入框的撤销/重做历史，同时保留节点与焦点。

## 属性

| 属性 / 方法 | 类型 | 说明 |
| --- | --- | --- |
| `DynamicForm::new(fields)` | `impl IntoIterator<Item = DynamicFormField>` | 用字段描述列表创建表单 |
| `.fields(fields)` | `impl IntoIterator<Item = DynamicFormField>` | 替换字段描述列表 |
| `.label(text)` | `impl Into<Arc<str>>` | 表单的无障碍名称（角色为 `Form`） |
| `.style(style)` | `NodeStyle` | 根节点样式 |
| `field_nodes()` | `&[(Arc<str>, StableNodeId)]` | 组装后按描述顺序返回每个字段（及分组卡片）的根节点 ID |

## 事件

`DynamicFormEvent` 只汇报字段键（`field: Arc<str>`）与新值，不替应用修改业务状态：

| 事件变体 | 载荷 | 说明 |
| --- | --- | --- |
| `DynamicFormEvent::Activate` | `{ field }` | 点击了 `Action` 按钮 |
| `DynamicFormEvent::Toggle` | `{ field, checked }` | 切换了 `Toggle` 开关 |
| `DynamicFormEvent::Select` | `{ field, value }` | 在 `Choice` 下拉框中选中了一项 |
| `DynamicFormEvent::Input` | `{ field, value }` | 在单行或多行 `Field` 中输入了文本 |

## 插槽

| 插槽 | 说明 |
| --- | --- |
| 无 | 分组卡片、设置行与内部控件均按字段描述自动创建并按键保留 |

## 参见

[PropertyGrid](property-grid.md) · [FormField](form-field.md) · [SettingsRow](settings-row.md) · [InteractionRequestCard](interaction-request-card.md) · [总览](index.md)
