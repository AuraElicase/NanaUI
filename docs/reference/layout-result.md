# 布局结果合同（LayoutResult）

`UiWorld` 为每个完成布局的节点发布一份不可变的 `LayoutResult`。它是面向绘制与输入的盒子几何权威；节点上的 `layout_box` 只是其 `bounds` 字段的兼容投影。

## 结构与代际

每份 `LayoutResult` 携带：

- 边框盒（border box）、内边距盒（padding box）与内容盒（content box）；
- 首行基线（first baseline）与末行基线（last baseline）；
- 溢出范围（overflow extent）与滚动范围（scroll extent）；
- 子节点放置坐标（child placements）与类型化的布局片段/部件（typed layout fragments / parts）；
- 裁剪（clip）与包含块（containing-block）引用、依赖元数据；
- 产生该结果的单调递增布局代际（`layout_generation`）。

Runtime 帧更新（`RuntimeFrameUpdate`）对外暴露这一 `layout_generation`，使 Scene 提取、命中测试、无障碍投影和滚动消费方能够对齐各自的投影版本，而不必依赖粒度更粗的 `UiWorld` 变更代际（mutation generation）。

`layout_generation` 独立于世界变更代际：**只有当布局输出实际变化时它才递增**。因此悬停（hover）、不透明度（opacity）、变换（transform）及其他纯绘制改动不会触发任何基于布局派生的几何重建。影响布局的写入会在下一次发布结果前清除受影响节点、后代及依赖祖先的缓存，而拓扑变更仍保持稳定的节点身份（`StableNodeId`）。

## 格式化上下文与子片段

`formatting_context` 记录该节点排列其子节点（或自身文本）时实际采用的格式化上下文（Issue #197），并决定每个子片段的具体类型：

| `formatting_context` | 子片段类型 |
| --- | --- |
| `Flex` | `FlexChildPlacement` |
| `Grid` | `GridChildPlacement` |
| `Inline` | `InlineAtomic` |
| `Flow` / `Overlay` | `ChildPlacement` |

在布局尚未记录上下文之前，仅由节点的 `display` 给出暂定上下文。若某次上下文切换碰巧让所有盒子留在原位置，仍然会发布一份新的 `LayoutResult`——因为上下文和片段类型已经改变；子节点的身份保持不变。

## 与 ComponentGeometry 的关系

`ComponentGeometry` 仍是组件语义部件与非盒式绘制数据（例如缩放拖柄、图表路径、编辑器装饰线）的兼容表面。在消费方逐步迁移到更丰富的 `LayoutResult` 字段期间，它的盒子位置由同一份 `LayoutBox` 投影锚定。

## 参见

- [布局引擎（架构学习）](/learn/runtime-layout) · [Runtime 与 Scene](runtime-scene.md) · [布局（Rust）](rust-layout.md) · [布局（CSS 子集）](layout.md)
