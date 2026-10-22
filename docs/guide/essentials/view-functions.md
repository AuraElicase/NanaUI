# 组合视图

一块界面是一个函数，返回 `impl IntoView`。`IntoView` 是能落成保留节点的东西：一个元素、若干视图组成的 tuple、`each`、`when`，以及 `()`。调用发生在挂载的时候，函数本身不缓存树。

模板里的组件是带 `#[component]` 的函数。`<Status label="空闲" />` 调用 `status("空闲")`。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::view::{IntoProp, IntoView, component};
use nana_ui::runtime::SemanticColorRole;

#[component]
fn status(label: impl IntoProp<String>) -> impl IntoView {
    view! {
        <Text foreground={SemanticColorRole::Muted}>{label}</Text>
    }
}

fn page() -> impl IntoView {
    view! {
        <Column gap=8>
            <Status label="空闲" />
        </Column>
    }
}
```

```rust rust
use nana_ui::runtime::view::*;
use nana_ui::runtime::SemanticColorRole;

fn status(label: impl IntoProp<String>) -> impl IntoView {
    text(label).foreground(SemanticColorRole::Muted)
}

fn page() -> impl IntoView {
    column().gap(8).children(status("空闲"))
}
```

:::

不认识的标签按 snake_case 找同名函数。`Status` 是 `status`，`TodoRow` 是 `todo_row`。属性按名字交给同名参数，顺序随意。子节点交给名为 `children` 的参数。没有子节点就不传。一个子节点就是那一个视图。多个子节点打成 tuple，超过 12 个再嵌套一层 tuple。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::view::{IntoView, component};

#[component]
fn frame(children: impl IntoView) -> impl IntoView {
    view! {
        <Column gap=8>
            {children}
        </Column>
    }
}
```

```rust rust
use nana_ui::runtime::view::{column, text, IntoView};

fn frame(children: impl IntoView) -> impl IntoView {
    column().gap(8).children(children)
}
```

:::

`<Frame><Text>"内容"</Text></Frame>` 就是 `frame(text("内容"))`。

## 可以不写的属性

参数上写 `#[prop(default)]`（取 `Default::default()`）或 `#[prop(default = 表达式)]`，模板就可以不写这个属性。只写属性名不写值（`<Setting disabled />`）就是 `true`。

:::api

```rust view
use nana_ui::runtime::view;
use nana_ui::runtime::view::{IntoProp, IntoView, component};

#[component]
fn setting(
    label: &'static str,
    #[prop(default = "")] hint: &'static str,
    #[prop(default = false)] disabled: impl IntoProp<bool>,
) -> impl IntoView {
    view! {
        <Column gap=2>
            <Checkbox label={label} disabled={disabled} />
            <Text>{hint}</Text>
        </Column>
    }
}

fn page() -> impl IntoView {
    view! {
        <Column gap=8>
            <Setting label="自动保存" />
            <Setting hint="需要先登录" label="同步" disabled />
        </Column>
    }
}
```

```rust rust
use nana_ui::runtime::view::*;

fn setting(label: &'static str, hint: &'static str, disabled: impl IntoProp<bool>) -> impl IntoView {
    column().gap(2).children((checkbox(label).disabled(disabled), text(hint)))
}

fn page() -> impl IntoView {
    column().gap(8).children((
        setting("自动保存", "", false),
        setting("同步", "需要先登录", true),
    ))
}
```

:::

默认值写在函数旁边，名字在函数所在的模块里解析，不在用它的模板里。`impl Trait` 参数也能有默认值，`#[prop(default = || true)] shown: impl Fn() -> bool` 这样写。类型用到函数自己泛型参数的属性不能有默认值。

这几种写法编译不过，错误指在模板上：

- 漏了没有默认值的属性：`` `<Setting>` needs `label` ``。
- 写了函数没有的参数：`no method named 'colour'`。同一个属性写两次也是错误。
- 函数没标 `#[component]`：编译器说找不到模块 `setting`。

`#[component]` 不改函数本身。Rust 里照常按顺序传齐参数，`setting("同步", "需要先登录", true)`。它在函数旁边加一个同名的隐藏模块，里面是模板展开后用的构建器（`setting::Props`）。`use` 进这个函数时，模块也一起进来了。参数要拿所有权或是 `'static` 的值。组件标签上的 `key` 另有用处（见下文），参数不能叫 `key`。

自定义标签只收这些值。事件和字段写在函数里面。`<Status @activate={save} />` 是模板错误：组件标签不能挂 `@` 或 `on:`。

带路径的标签一律调用函数，就算最后一段和内置标签同名。不带路径时，内置标签优先。你自己的函数如果也叫 `button`，会被内置的 `Button` 挡住。同模块里写成 `<self::Button />` 才调用你的函数。

写在组件标签上的 `key` 落在它第一个根节点上，展开成 `keyed(key, 调用)`。

## 返回不同类型

`impl IntoView` 在一个函数里只能是一个具体类型。两条分支类型不同时，用 `into_any` 擦成 `AnyView`。`AnyView` 自己也实现 `IntoView`。

```rust
use nana_ui::runtime::view::{column, text, IntoView};

fn body(empty: bool) -> impl IntoView {
    if empty {
        text("还没有").into_any()
    } else {
        column().gap(8).children(text("有内容")).into_any()
    }
}
```

`Option<视图>` 也是视图。`None` 什么也不建。`show.then(|| text("在"))` 在 `show` 为假时就是空的。

::: warning
函数里的 `if` 在这次调用时决定结构，之后不再跟着数据变。`body` 要跟着信号换，用 [条件](conditional.md) 里的 `when`，不要在函数里 `if signal.get()`。`signal.get()` 在构造期间是当时的常量。
:::

`()` 是空视图。tuple 里可以夹一个 `()`，占一个位置但不建节点。写死的几个子节点优先用 tuple，`.children((a, b))`。要在建树时写 `for` 或 `let`，用 `.with(|c| c.add(…))`，这个块也只跑一次。

内置标签不会去调用你的函数。`Column`、`Row`、`Text`、`Button` 这些展开成 `column()`、`text`、`button`，见 [视图写法](view.md) 的对照表。

## 参数从哪来

父视图把信号、路径句柄或克隆出来的值传进来。行视图从 `each` 收到那一项的克隆，所以 `title: String` 这种参数在行里面是自己的。要跟着原数据变，传信号、`Computed`，或 [Store](reactivity.md) 的字段句柄，不要传 `get()` 出来的副本。

回调是普通参数：`fn toolbar(start: impl FnMut() + Send + 'static) -> impl IntoView`，里面 `button("开始").on_activate(start)`。没有单独的 emits。模板里这仍然是一个属性，不是 `@`。

## 接下来

<div class="next-steps">
  <a class="next-step" href="/guide/essentials/lifecycle">
    <p class="next-step-link">挂载与卸载</p>
    <p class="next-step-caption">这些函数在哪个作用域里求值，卸载时留下什么。</p>
  </a>
  <a class="next-step" href="/guide/essentials/view">
    <p class="next-step-link">视图写法</p>
    <p class="next-step-caption">内置标签怎样展开成函数调用。</p>
  </a>
</div>
