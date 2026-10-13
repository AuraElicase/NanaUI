# FindReplaceBar

`FindReplaceBar`（别名 `TextSearchBar`）提供可折叠的查找与替换工具条。它只维护查询/替换草稿、展开状态、只读门控与反馈文字；搜索文档、定位匹配项和写回修改由应用处理。查询框和替换框是保留的 `TextInput` 子节点，因此输入法（IME）、选区和无障碍都走标准输入路径。

控件表里没有 `<FindReplaceBar>` 模板标签，两种写法都用 `widget(FindReplaceBar::new())` 挂载。

## 基本用法

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{FindReplaceBar, FindReplaceEvent};

widget(
    FindReplaceBar::new()
        .query("nana")
        .expanded(true)
        .feedback("1 / 3"),
)
.on(|event: &FindReplaceEvent| {
    // 处理 QueryChanged / Next / Replace 等意图
})
```

```rust rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{FindReplaceBar, FindReplaceEvent};

widget(
    FindReplaceBar::new()
        .query("nana")
        .expanded(true)
        .feedback("1 / 3"),
)
.on(|event: &FindReplaceEvent| {
    // 处理 QueryChanged / Next / Replace 等意图
})
```

:::

手工通过 `create_component` 创建时，需调用 `cx.assemble_find_replace_bar(bar)?` 组装内部保留子节点；放在声明式视图（`widget(...)`）里时由框架自动组装。

## 行为与只读门控

- **折叠与展开**：默认折叠（只显示切换按钮）；点击切换按钮或设 `.expanded(true)` 展开查询行与替换行，并发出 `FindReplaceEvent::Expanded(bool)`。
- **只读模式（`read_only`）**：保持查询输入和「上一个 / 下一个」导航可用，同时禁用替换输入框以及「替换 / 全部替换」按钮。
- **整体禁用（`disabled`）**：禁用全部输入框和按钮，激活不再发出事件。
- **默认文案**：占位符、按钮文字和无障碍名称默认读取 `FrameworkStrings` 的 `find.*` 条目（见 [框架文案](../reference/framework-strings.md)），也可通过 `.query_placeholder(...)`、`.replacement_placeholder(...)`、`.label(...)` 单独覆盖。

## 属性

| 属性 / 方法 | 类型 | 说明 |
| --- | --- | --- |
| `FindReplaceBar::new()` | — | 创建默认折叠、可读写的查找替换条 |
| `.query(text)` | `impl Into<String>` | 当前查找词草稿 |
| `.replacement(text)` | `impl Into<String>` | 当前替换词草稿 |
| `.expanded(bool)` | `bool` | 是否展开面板，默认 `false` |
| `.read_only(bool)` | `bool` | 只读文档门控：保留查找与跳转，禁用替换，默认 `false` |
| `.disabled(bool)` | `bool` | 禁用整个工具条，默认 `false` |
| `.feedback(text)` | `impl Into<Arc<str>>` | 结果反馈文字（例如 `"2 / 5"` 或 `"无匹配"`） |
| `.query_placeholder(text)` | `impl Into<Arc<str>>` | 查找框占位符，缺省使用 `FrameworkStrings::find_query` |
| `.replacement_placeholder(text)` | `impl Into<Arc<str>>` | 替换框占位符，缺省使用 `FrameworkStrings::find_replacement` |
| `.label(text)` | `impl Into<Arc<str>>` | 工具条的无障碍名称（角色为 `Toolbar`），缺省使用 `find.title` |
| `.style(style)` | `NodeStyle` | 根节点样式 |
| `toggle_id()` / `query_input_id()` | `Option<StableNodeId>` | 组装后读取切换按钮或查找输入框的节点 ID（例如用于主动聚焦） |

## 事件

组件通过 `FindReplaceEvent` 汇报用户意图：

| 事件变体 | 载荷 | 说明 |
| --- | --- | --- |
| `FindReplaceEvent::QueryChanged(String)` | 当前查找词 | 查找输入框内容变化 |
| `FindReplaceEvent::ReplacementChanged(String)` | 当前替换词 | 替换输入框内容变化 |
| `FindReplaceEvent::Previous` | 无 | 点击「上一个」 |
| `FindReplaceEvent::Next` | 无 | 点击「下一个」 |
| `FindReplaceEvent::Replace` | 无 | 点击「替换」（`read_only` 时不触发） |
| `FindReplaceEvent::ReplaceAll` | 无 | 点击「全部替换」（`read_only` 时不触发） |
| `FindReplaceEvent::Expanded(bool)` | 是否展开 | 用户点击切换按钮展开或收起面板 |

## 插槽

| 插槽 | 说明 |
| --- | --- |
| 无 | 切换按钮、查询/替换输入框、反馈文字和操作按钮均由组件内部保留并组装 |

## 参见

[TextInput](text-input.md) · [SearchDropdown](search-dropdown.md) · [总览](index.md) · [控件](../reference/components.md)
