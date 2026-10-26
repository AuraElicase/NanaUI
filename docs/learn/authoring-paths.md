# 作者入口

NanaUI 提供两种编写界面的方式：Rust（`nana_ui::runtime` 构建器与 `view!` 宏）和 `.vue` 方言（脚本为 Rust 的单文件组件）。两者在构建期都变成普通的 Rust 视图函数，底层始终只有**一棵保留树（`UiWorld`）**和**一个绘制器（`SceneWgpuPainter`）**。本章拆解两条入口各自的链路、在哪里汇合，并给出「为什么不会长出第二棵树」的源码级证据。

## 全局汇合图

```text
入口一（Rust）       view! / css! / stylesheet! 与函数式构建器
                        │ nana-ui-view-macros → view-codegen（构建时调用 CSS 引擎）
入口二（.vue 方言）  <script setup lang="rust"> 单文件组件
                        │ nana-ui-sfc（应用 build.rs）→ view-codegen（构建时调用 CSS 引擎）
                        ▼
        Runtime 视图函数 + StylePatch 数据
                        │ MutationQueue（nana-ui-runtime 的 UiMutation 词表）
                        │ UiWorld::commit（整批校验 + 原子应用）
                        ▼
        UiWorld（nana-ui-runtime，唯一权威保留树）
                        ▼
        UiScene → SceneWgpuPainter（nana-ui，唯一绘制器）
```

两条入口是**两种书写形式**，而非两个独立运行时：它们的样式统一汇入同一份 Style Model（Tokens + Semantics + Layout），节点全部写入同一棵 `UiWorld`。

## 入口一：Rust（`view!` 宏链与函数式视图）

在 Rust 路径中，既可以使用 `<Tag>` 模板语法，也可以使用等价的函数式构建器（如 `column().gap(8).children(...)`）：

```rust
use nana_ui::runtime::view;

view! {
    <Column gap=8>
        <Button @activate={save}>"确定"</Button>
    </Column>
}
```

构建时编译链路如下：

| 阶段 | Crate | 职责说明 |
| --- | --- | --- |
| 1. 过程宏入口 | `nana-ui-view-macros` | 解析 `view!`、`css!`、`stylesheet!` 语法树 |
| 2. 控件描述表 | `nana-ui-view-schema` | 提供内置控件标签、字段与事件的唯一中间表示（`for_each_control!`） |
| 3. 代码生成 | `nana-ui-view-codegen` | 展开为 Runtime 视图函数调用；`<style>` 与 `css!` 中的 CSS **在构建时**调用 `nana-ui-css` 解析，直接生成 Style Model 补丁数据 |

源码证据：

- `crates/nana-ui-runtime/src/view/mod.rs:151-162`：`stylesheet!` 宏在 Runtime 中定义并转发给 `nana_ui_view_macros`：
  ```rust
  macro_rules! stylesheet { ... }
  pub use nana_ui_view_macros::stylesheet as __stylesheet;
  ```
- 运行时拿到的是**纯数据编译产物**：`crates/nana-ui-runtime/src/view/style.rs:3-8` 模块文档明确指出 `.vue` 编译器与 `css!` 在构建时解析并匹配视图样式表，运行时不解析 CSS、不匹配选择器，本 crate 不含 CSS 引擎。
- 依赖图佐证：`nana-ui-runtime` 与 `nana-ui` 的普通依赖表中均没有 `nana-ui-css`，仅有 `view-macros` 与 `view-schema`（详见 [模块与分层](/learn/modules-and-layers)）。

## 入口二：`.vue` 方言（构建期编译为 Rust）

`.vue` 单文件组件的脚本是 Rust（`<script setup lang="rust">`）。`nana-ui-sfc` 在应用的 `build.rs` 中把它编译为标准 Rust 视图函数（例如 `TodoList.vue` 生成 `pub fn todo_list(...) -> impl IntoView`），模板与 `<style>` 交给同一个 `nana-ui-view-codegen` 展开，产物写入 `$OUT_DIR/nana_views.rs`（详见 [作者工具链](/learn/toolchain)）：

- 生成代码通过路径字符串引用 `nana_ui::runtime`，与手写 `view!` 落在同一组视图函数上；
- `<style>` 中的 CSS 同样**在构建时**由 `nana-ui-css` 解析并匹配，运行时拿到的是 `StylePatch` 数据（`crates/nana-ui-runtime/src/view/style.rs:3-9`）。

## 汇合点：`UiMutation` 与 `UiWorld::commit`

`UiMutation` 词表定义在 **`nana-ui-runtime`**（`crates/nana-ui-runtime/src/mutation.rs:15`，`MutationQueue` 位于 `:306`）：`Create`、`Insert`、`Detach`、`SetStyle`、`StartAnimation`、`RequestFocus`……视图函数与组件装配器的每一项写入都是 Runtime 已经定义好的标准变异，由 `UiWorld::commit` 整批校验后原子应用（见 [运行时保留树](/learn/runtime-tree)）。两条入口都没有属于自己的第二套树操作原语。

## 「只有一棵树」核查读数

| 潜在破坏条件 | 当前工作区实测结果 |
| --- | --- |
| 某条入口维护独立的保留树结构 | **无。** 两条入口在构建期都变成 Runtime 视图函数，写入只经 `MutationQueue` 进入 `UiWorld`。 |
| 某条入口维护独立的绘制管线 | **无。** 绘制统一走 `nana-ui` 的 Scene 宿主（`UiWorld → UiScene → SceneWgpuPainter`）。 |
| 存在第二套组件实例化 ABI | **无。** 内置与扩展组件统一走 `ComponentRegistry`。 |

## 给学习者的两个观察点

1. **找汇合点比找分岔点更重要**：两条入口在 `UiMutation` / `UiWorld` 完全汇合，因此只要掌握一遍保留树 API，两条路径的内部行为就全部贯通。
2. **CSS 只在构建时解析**：同一个引擎（`nana-ui-css`），由 `view-codegen` 在构建期为 `css!`、`stylesheet!` 与 `.vue` 的 `<style>` 调用；运行时只接收 Style Model 数据。

## 接着读

- [保留树：UiWorld](/learn/runtime-tree)：两条入口汇合后的节点表与提交机制
- [CSS 引擎](/learn/css-engine)：构建期调用的内部流水线
- [端到端数据流](/learn/flows)：一次状态更新从模板到屏幕像素的完整旅程
- [.vue 方言（指南）](/guide/scaling/sfc)：面向应用开发者的 `.vue` 方言指南
