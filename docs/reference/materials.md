# 通用片元材质

`nana-ui-core` 的 `MaterialProgram`、`MaterialRef` 与 `MaterialTexture` 是文字与普通形状共用的绘制合同。它们不包含设备、窗口或应用状态。`PaintStyle.background_material`、`border_material`、`text_material` 引用同一种材质；`RichPaintStyle.material` 可覆盖节点的文字材质。

本阶段适配普通 Quad 的填色与边框（含圆角、polygon clip），以及 Text / RichText 的字形填色。Path、自定义 Painter、图标、彩色 emoji 和内嵌图片继续使用各自的绘制路径。阴影、文字描边和装饰线保留原样。

## 程序与实例

作者提供 WGSL 纯函数，不提供顶点阶段或绑定：

```wgsl
fn material(i: MaterialInput) -> vec4<f32> {
    let stripe = select(0.4, 1.0, fract(i.position.x / 4.0) < 0.5);
    return vec4<f32>(i.color.rgb, stripe);
}
```

另一种材质可直接使用实例参数，程序合同相同：

```wgsl
fn material(i: MaterialInput) -> vec4<f32> {
    return vec4<f32>(material_param(0u).rgb, 1.0);
}
```

`MaterialInput` 的 `position` 是局部逻辑像素，`uv` 是局部归一化坐标，`size` 是局部逻辑尺寸，`color.rgb` 是已解析的线性 scRGB 颜色。形状的局部区域是完整框体，`glyph_uv` 为零；文字的局部区域是当前字形单元，`glyph_uv` 与 `uv` 均为该单元内坐标。坐标在 presentation 变换前取得，笔触会随文字移动而不在窗口坐标上滑动。

颜色输入的 alpha 为 1，材质输出的 alpha 表示额外的不透明度。框架将其限制到 0–1，再乘入原始颜色 alpha、字形或形状覆盖、揭示、裁剪、祖先不透明度；材质无法画出字形或轮廓外。RGB 不做 SDR 截断，框架负责预乘和最终呈现转换。显式 span 颜色及彩虹动效先解析，再交给文字材质。

`material_param(index)` 读取 vec4 参数；越界索引取最后一组，空参数使用零值。最多 256 组有限 vec4。`material_sample0(uv)` / `material_sample1(uv)` 采样最多两张直通 alpha sRGB 纹理并返回线性 RGBA。UV 限制到 0–1。纹理最大边长为设备限制与 4096 中较小者，RGBA 字节数必须与尺寸吻合。

Rust 调用方使用 `MaterialProgram::new(wgsl)` 和 `MaterialRef::new(program)`，再通过 `with_parameters` / `with_textures` 构造不可变实例。克隆共享引用；替换参数或纹理保留实例身份并生成新的修订。序列化后重新加载会产生新的运行时身份。

## 生命周期与回退

材质资源在首次绘制时创建，同一设备上的文字和形状共享参数、纹理布局与实例绑定。程序缓存区分设备代际、适配器类型、目标格式、采样数和程序内容；无活动引用的程序与绑定在后续准备阶段清理。设备替换后按新代际重建。

材质不隐式订阅时钟或请求帧。动态效果由调用方更新参数并使用既有 presentation/帧调度合同。参数改变只使绘制失效，复用文本塑形与字形图集；稳态不重传字形。

编译失败或资源不合法时使用原始样式。驱动编译错误通过 `gpu.material.rejected` 诊断事件报告。程序由宿主应用提供；加载第三方程序的应用仍负责源码、复杂度与资源预算校验，不能把驱动校验当作不可信代码沙箱。

GPU 证据位于 `nana-ui-devtools/tests/materials.rs`，涵盖填色、独立边框、富文本覆盖、纹理替换、静态确定性、恒等材质及失败回退。文字参数复用测试位于 text 模块，设备恢复测试覆盖保留场景中的文字材质。

## 通用呈现遮罩 PresentationMask

`PresentationMask` 是内容无关的单个绘制对象覆盖率合同：材质引用、开始时间、有限时长以及局部／目标坐标空间。`progress(now)` 返回精确的 0–1 进度，`PresentationMaskSample` 携带采样进度及对象局部坐标中的目标矩形。它不拥有文字、字形图集、裁剪或颜色，也不规定播放业务。

遮罩复用 `MaterialRef` 和 `fn material(i: MaterialInput) -> vec4<f32>`，只消费返回值的 alpha，钳到 0–1 后乘入原绘制覆盖率；RGB 保留原管线结果。`progress`、`target_position` 和 `target_size` 是新增公共输入。局部坐标为变换前的逻辑像素；目标坐标始终相对于目标矩形。普通颜色材质的进度为 1，旧 WGSL 无需修改。

文字是该合同的一个适配层：`GlyphPresentation.transitions` 分别引用入场／退场遮罩，入场按 `RevealSchedule` 的逐字时刻采样进度，退场使用独立的有限时钟。`TextTransitions.freeze_reveal` 冻结旧文字的揭示时刻，`viewport` 可指定文字局部的可见视口，省略时由绘制器推导。`Local` 使用字形或行内对象坐标，`Target` 使用整个正文视口坐标；额外的 `glyph_uv` 为文字适配信息，不是通用遮罩的前提。

字形填色、描边、阴影、装饰线、ruby 和彩色 emoji 应用相同遮罩。行内图片／标签的 Quad 通过 `QuadSurfacePaint.presentation_masks` 使用同一 `PresentationMaskSample` 合同，保留原始图像色彩；首轮支持两个遮罩叠加。动画只更新 presentation 或 paint 数据，复用文字布局、图集及材质资源；截止时间结束后停止连续帧。

本轮只实现单个绘制对象，不对任意子树做后处理。未来容器可先合成子内容，将合成结果作为一个绘制对象，继续使用同一局部坐标、目标坐标和覆盖率语义；当前没有容器合成占位实现。NanaType 的逐字节奏、换句等待和蜡笔算法均不属于框架。
