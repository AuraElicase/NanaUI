# Texture

`texture()` 在布局盒子里显示一块宿主纹理。模板里的标签是 `<Texture>`，类型是 `GpuTextureView`。它没有构造参数。

## 基本用法

用来放视频帧和预览，不加载网页。字段是 `resource: TextureSlotName`、`opacity: f32`、`corner_radius: f32`。没有事件，也没有 `model`。`resource` 是宿主纹理登记的槽名，十进制 id 也可以，只要宿主用这个键登记过。

:::api

```rust view
use nana_ui::runtime::view;

view! {
    <Texture resource={frame} corner_radius=8 />
}
```

```rust rust
use nana_ui::runtime::view::texture;

texture().resource(frame).corner_radius(8_f32)
```

:::

模板里的 `corner_radius=8` 展开成 `8_f32`。手写的 `f32` 字面量要带后缀。`opacity` 同样是 `f32`，不写时是完全不透明。空的 `resource` 不挂场景节点，避免空白身份被拒绝。

## 换图

换图只在宿主那边做：往同一个槽重新登记纹理（新像素、换了视图、设备重建后重新上传），所有显示这个槽的视图下一帧就采样到新纹理，不用动组件。组件上没有修订号，所以同一个槽可以同时给几个视图用。

默认只采样第 0 层，宿主按 `painted_extent` 准备尺寸。有 mip 链时，`.sampling(ImageSampling::Mipmap)` 改三线性。

## 铺放和命中

默认铺满盒子（`ContentFit::Fill`），不参与命中。要命中，用组件上的 `with_pointer_events`。要留边，用 `contain`。这两项不在字段表里。圆角字段是四角同一个半径；只要上面两角之类的形状，用组件的 `with_corner_radii`。

投影时透明度夹到 0 到 1，非有限值按 1。圆角非有限或负数按 0。棋盘底和放大在组件上：`checkerboard`、`zoom`。`zoom` 小于 1 或非有限时按 1，也就是适配大小，不缩小到盒子里面再留一圈。

## 网页与场景键

`GpuTextureView` 不是浏览器。应用内网页用 `runtime::BrowserView`，它不是这个类型的别名。当前只有 macOS 的 `WKWebView`，Windows 和 Linux 明确不可用。

场景键是 `"nana.host-texture"`，不是另一套画家，也不是 `"gpu-texture-view"`。`Thumbnail` 和 `Avatar` 用同一个登记。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `resource` | `TextureSlotName` | 宿主纹理登记的槽名，十进制 id 也可以，只要宿主用这个键登记过。空的不挂场景节点，避免空白身份被拒绝 |
| `opacity` | `f32` | 不写时是完全不透明。投影时夹到 0 到 1，非有限值按 1 |
| `corner_radius` | `f32` | 四角同一个半径。模板里 `corner_radius=8` 展开成 `8_f32`。非有限或负数按 0 |

## 事件

没有事件，也没有 `model`。

## 插槽

没有插槽。画面来自 `resource` 指向的宿主纹理。

## 参见

[总览](index.md) · [控件](../reference/components.md)
