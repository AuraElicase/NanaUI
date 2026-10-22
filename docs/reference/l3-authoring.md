# L3：用 Rust 构建保留式界面

本文说明 Rust 声明式视图（L3）的挂载方式、节点身份（Key）规则、细粒度更新合同以及与即时模式 / GPUI 类框架的本质区别。首次编写窗口请先看 [快速开始](../guide/quick-start.md) 与 [创建应用](../guide/essentials/application.md)。

Rust 应用通过 [声明式视图](reactive-view.md) 构建界面：视图是一个普通 Rust 表达式（可用 `view!` 宏或等价的函数调用编写），**只在挂载时求值一次**并批量提交（commit）进保留的 `UiWorld`；后续状态变化仅通过信号（`Signal` / `Store`）或组件句柄定点更新受影响的字段，绝不每帧重跑整棵视图函数。

## 挂载与句柄提取

```rust
let (view, start) = cx.mount_view_root(document_id, || {
    let start = entity_ref::<Button>();
    let page = column().gap(12).children((
        text("你好"),
        button("开始")
            .entity_ref(start)
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(Start)),
        row().gap(8).children((button("打开"), button("浮窗"))),
    ));
    with_refs(page, start)
})?;
```

常用挂载与组合入口：

| 入口 / 方法 | 作用 |
| --- | --- |
| `mount_view_root(document, \|\| view)` | 将视图根节点作为文档根挂载 |
| `mount_view(parent, \|\| view)` | 将视图挂载到已有父节点下（根自身不占用父节点的组装 key） |
| `mount_view_detached(document, \|\| view)` | 先在文档内创建游离视图，供复合控件按 ID 放入对应槽位 |
| `with_refs(view, refs)` | 挂载完成后直接返回 `(MountedView, 解析好的句柄)`，将 `EntityRef<C>` 转为 `Entity<C>` |
| `.on(\|e\| …)` / `.on_cx(\|组件, e, cx\| …)` | 就近绑定事件；`on_cx` 可原地修改组件状态或调用 `cx.dispatch_program` |
| `.slot` / `.child_slot` / 具名 slot | 向复合控件（如 Shell 区域、分组工具栏、列表行图标）传入子视图 |
| `#[component]` | 让模板按属性名调用应用自己的视图函数（`<SettingsRow label="…" on={…}/>`），`#[prop(default = …)]` 的参数可以不写；Rust 里仍按顺序调用同一个函数 |

在声明式视图中，`Stack`（`column()` / `row()`）仅作为样式与排版容器；复合控件在挂载时会自动执行各自的装配器（无需手动调用 `assemble_*`）。

## 节点身份（Key）与路径解析

- **静态结构按位置命名**：未显式指定 `.key("…")` 的静态节点自动按位置分配内部键；只有在需要通过路径查找节点或向按键接收子节点的容器（如 `Dock`、`AreaLayout`、`PropertyGrid`）传内容时，才需要显式写 `.key("…")`。
- **Key 的合法性约束**：同一个父节点下 key 重复会返回 `DuplicateAssemblyKey`；key 为空字符串或包含路径分隔符 `/`（`ASSEMBLY_PATH_SEPARATOR`）会返回 `InvalidInput`，整批拒绝提交。
- **按路径查找**：视图中任何带 key 的节点都可从视图根按相对路径解析：`resolve_assembly_path(root, "content/list")` 或带类型的 `resolve_assembly_entity::<C>(root, path)`。如果只是想在挂载处拿到某个控件的 `Entity<C>`，优先使用 `entity_ref` + `with_refs`。
- **动态子列表**：建树时的一次性循环可用 `.children(items.iter().map(…).collect::<Vec<_>>())` 或 `.with(|c| for … { c.add(…) })`；需要随响应式数据增删或重排的列表则使用 `each`、`each_virtual` 或条件分支 `when`。

## 细粒度更新（不要整树重建）

| 变更场景 | 正确做法 |
| --- | --- |
| 控件字段随响应式状态变化 | 直接绑定信号或闭包：`.label(sig)`、`.bind(\|c\| …)`、`text!("{count}")` |
| 在消息循环（`RuntimeProgram::update`）中改单个控件 | 挂载时用 `entity_ref` / `with_refs` 留住句柄，随后调用 `update_component(entity, \|v, _\| …)` |
| 动态增删子项或切换分支 | 使用 `each` / `when`（底层命令式 `cx.mount(parent, …)` 仍可用于复合控件内部装配） |
| 大规模长列表 / 树 / 表格 | 使用 `each_virtual`（底层对应 `materialize_virtual_*_retained`） |

::: warning
切勿在 `RuntimeProgram::update` 或点击回调里卸载再重新 `mount_view_root` 整棵树。如果需要开发期改代码后快速预览，请参考 [开发期热重载](hot-reload.md)。
:::

## 作者面 API 分工

| API | 定位与适用场景 |
| --- | --- |
| 声明式视图（`view!` / 函数式视图） | **产品默认作者面**：应用界面与复合视图首选 |
| `create_component` / `append_child` / `on` | 底层原语：用于单元测试、基准测试或单节点命令式补丁 |
| `mount`（`AssemblyScope`） | 命令式 keyed 装配：供框架内置复合控件的 `assemble_*` 使用 |
| `bind_component` | 仅供 Vue 宿主或已有固定 ID 的桥接层使用 |
| `RuntimeProgram` / `ApplicationState` | 宿主生命周期与跨窗口/GPU 消息编排，与声明式视图配合使用 |

## 性能特征与同树对比

一次 `mount_view_root` 对应一次原子的 `world.commit`（包含全部节点的创建、投影与父子插入），挂载阶段不触发文本整形、布局求解、Scene 抽取或 GPU 上传。相比之下，循环调用命令式的 `create_component` + `append_child` 每次都会单独 commit，因此整页构建务必使用声明式视图或批量队列。

### Rust L3 与 Vue L1/L2 的实测对比

由于两条路径写入同一棵 `UiWorld`，布局、文本整形、命中测试、Scene 抽取与 WGPU 绘制完全共享同一份 Rust 实现，**稳态渲染性能没有任何差异**（60 秒、21 万帧纯 Rust 场景下 CPU prepare 的 P95 为 `0.0105 ms`）。两者的差异仅体现在两个边界上：

- **首次挂载与大批量结构变更**：Vue 路径多出 V8 虚拟 DOM patch、每次 DOM host op 的跨界调用以及运行时 CSS 级联。5,000 节点构造的 P95 约为 `25.5 ms`，而同规模 Runtime 布局的 P95 为 `11.6–29.7 ms`（同一数量级；无论哪条路径，超过 5,000 节点都应使用虚拟滚动或分批挂载）。
- **单次指针移动事件**：两条路径均为 $O(1)$ 常数开销，不随树规模增长——Rust L3 约为 `0.0003 ms`，Vue 约为 `0.062 ms`（详见 [输入成本](input-cost.md)）。

### 与 GPUI 类框架的差异

GPUI 等框架采用「`Entity` 持有状态 + 每帧或每次 `cx.notify()` 重新执行 `render()` 返回整棵 `div().child(...)` 树」的模型。NanaUI 的声明式视图虽然同样写成直观的嵌套表达式并就近绑定事件，但**只在挂载时执行一次**：运行时是带稳定 `StableNodeId` 的保留树 `UiWorld`，状态变化通过细粒度信号绑定直接写进对应节点的属性槽位。

因此在 NanaUI 中请避免以下反模式：
- 不要发明 `div().px_4().bg_red()` 式的链式原子样式 API（与统一的 Style Model 冲突；需要写样式表时直接使用构建时编译的 `<style>`、`stylesheet!` 或 `css!`）；
- 不要每帧重跑视图构造函数，也不要把事件闭包塞进组件结构体字段（`ComponentView` 要求可 `Clone` 的纯状态快照，事件处理器由 `AppContext` 统一管理）。
