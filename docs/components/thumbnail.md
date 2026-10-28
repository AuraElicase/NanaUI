# Thumbnail

`thumbnail()` 创建一个宿主纹理的封面槽。模板里的标签是 `<Thumbnail>`。它没有构造参数，内部是 `Thumbnail::new("")`。

## 基本用法

空的 `resource` 是空态，不采样。模板字段是 `resource`、`aspect`、`label`、`decorative`、`pickable`、`intrinsic_size`。可拾取时有 `@pick` 事件，没有 `model`。

:::api

```rust view
use nana_ui::runtime::view;

view! {
    <Thumbnail resource={cover} aspect=1.5 label="封面" />
}
```

```rust rust
use nana_ui::runtime::view::thumbnail;

thumbnail().resource(cover).aspect(1.5_f32).label("封面")
```

:::

模板里的 `aspect=1.5` 展开成 `1.5_f32`。手写时字段是 `f32`，字面量要带后缀。没写 `aspect` 时按 1。盒子是控件高度乘这个比。显式样式的宽高、约束和圆角优先。圆角都没写时才取 `Xs`。非有限或非正的 `aspect` 退回 1。

默认适配是 `ContentFit::Contain`。封面裁切用组件上的 `Thumbnail::fit(ContentFit::Cover)`，不是字段。空、加载、就绪、不可用共用这一套布局尺寸。

## 状态

状态是组件上的 `ThumbnailState`：`Empty`、`Loading`、`Ready`、`Unavailable`。不在字段表里。`resource` 去掉空白后非空，构造时就是 Ready，否则是 Empty。`loading()` 和 `unavailable()` 用来标另外两态。四态共用盒子，所以加载时布局不跳。

Loading 的转圈居中，边长是紧凑 `Spinner` 的两倍。

## 换图

换图只在宿主那边做：往同一个槽重新登记纹理（新像素、换了视图、设备重建后重新上传），所有显示这个槽的视图下一帧就采样到新纹理，不用动组件。组件上没有修订号，所以同一个槽可以同时给几个视图用。

默认只采样第 0 层，宿主按 `painted_extent` 准备尺寸。有 mip 链时，组件上 `.sampling(ImageSampling::Mipmap)` 改成三线性。

## 命中、角标和名字

指针默认不参与命中，命中归父行。

## 在图片上拾取

`pickable` 打开后，控件自己接指针，按图片本身的坐标报 `ThumbnailPick`：`(0, 0)` 是图片左上角，`(1, 1)` 是右下角，和盒子尺寸、适配方式无关。宿主拿这个点去查自己的像素数据（例如按图片像素的归属图找到点中的区域）。

- `Hover(Some(点))`：指针在图片上移动，每次移动都报。
- `Hover(None)`：指针离开图片（进入 Contain 的留边、移出盒子、或按下被取消），每次离开只报一次。
- `Press { point, modifiers }`：主键在图片上按下。留边上按下不报，也不占用指针，交给父级。

Contain 的留边要知道图片的像素尺寸才能算，宿主用 `intrinsic_size` 写上槽里纹理的宽高。不写时整个盒子当作图片，只有 `Fill` 或盒子与图片同比例时才准。Cover 裁掉的部分不可拾取。

控件不取焦点。用指针拾取的选项，宿主要另外给一份键盘可达的列表（例如逐项开关）。

:::api

```rust view
use nana_ui::runtime::{ThumbnailPick, view};

view! {
    <Thumbnail resource={page} pickable=true intrinsic_size={Some((w, h))}
        label="纹理页" @pick={move |pick: &ThumbnailPick| on_pick(*pick)} />
}
```

```rust rust
use nana_ui::runtime::ThumbnailPick;
use nana_ui::runtime::view::thumbnail;

thumbnail()
    .resource(page)
    .pickable(true)
    .intrinsic_size(Some((w, h)))
    .label("纹理页")
    .on_pick(move |pick: &ThumbnailPick| on_pick(*pick))
```

:::

组件上的写法是 `Thumbnail::new(page).intrinsic_size(w, h).pickable()`；`Thumbnail::picture_point` 给出同一套换算，供宿主自己的测试或别的输入路径使用。

角标用 `Thumbnail::badge()`：右下、不命中、实底，裁在这个盒子的圆角里。角标是 `Thumbnail::badge()`，自己也关掉命中。

有 `label` 时无障碍是有名字的 Image。名字为空则没有 name。和 `Avatar` 的区别：这里默认是 Contain、方框随高度和宽高比，不是圆形固定边长。

重复或仅用于装饰的封面层可以用 `Thumbnail::decorative()`，模板中用 `decorative=true`。它仍然绘制并参与布局，但会从无障碍树中移除，等价于 `aria-hidden="true"`；因此只有最前面的封面层应保留 `label`。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `resource` | `Arc<str>` | 空的是空态，不采样。去掉空白后非空，构造时就是 Ready，否则是 Empty |
| `aspect` | `f32` | 没写时按 1。模板 `aspect=1.5` 展开成 `1.5_f32`。非有限或非正退回 1。盒子是控件高度乘这个比 |
| `label` | `Arc<str>` | 有 `label` 时无障碍是有名字的 Image。名字为空则没有 name |
| `decorative` | `bool` | 仍绘制并参与布局，但从无障碍树中移除；用于重复或装饰层 |
| `pickable` | `bool` | 控件自己接指针，按图片坐标报 `@pick`。默认关，命中归父级 |
| `intrinsic_size` | `Option<(u32, u32)>` | 槽里纹理的像素尺寸，用来算适配后的图片位置。不写时整个盒子当图片 |

## 事件

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| `@pick` | `ThumbnailPick` | 只在 `pickable` 时发出：图片上的悬停、离开与主键按下，坐标是图片内的 `0..=1` |

没有 `model`。

## 插槽

没有插槽。画面来自 `resource` 指向的宿主纹理。

## 参见

[总览](index.md) · [控件](../reference/components.md)
