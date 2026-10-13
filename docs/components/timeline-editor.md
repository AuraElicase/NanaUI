# TimelineEditor

`TimelineEditor` 是可编辑的时间底座，给动画、音频或数据编辑用。一个节点里画出时间标尺和播放头、左侧可折叠的轨道树、轨道上的关键点，以及曲线视图里的贝塞尔曲线和切线手柄。内容归应用：应用填 `tracks`，编辑通过事件回报，应用改好数据再写回。它没有模板标签，用 `widget(TimelineEditor::new(轨道))` 放进视图。

> 动画里的 `Timeline`（动作描述）是另一样东西；这个组件叫 `TimelineEditor`，它的数据和事件类型以 `Timeline` 开头。

## 基本用法

```rust
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{
    TimelineEditor, TimelineKey, TimelineKeysMoved, TimelinePlayheadChanged, TimelineTrack,
};

widget(
    TimelineEditor::new([
        TimelineTrack::new("head", "头", []).expanded(true),
        TimelineTrack::new("x", "角度 X", [TimelineKey::new(0.0, 0.0), TimelineKey::new(1.0, 30.0)])
            .depth(1),
        TimelineTrack::new("blink", "眨眼", [TimelineKey::new(0.8, 1.0)]),
    ])
    .fps(30.0)
    .playhead(0.5),
)
.on(|e: &TimelinePlayheadChanged| { /* e.time，e.committed */ })
.on(|e: &TimelineKeysMoved| { /* 松手（e.committed）时把 e.keys 移动 e.dt 秒、e.dv */ })
```

## 标尺与播放头

- 刻度单位是帧（`TimelineUnit::Frames`，按 `.fps()`）或秒。带数字的刻度至少相隔 64 px，取 1 / 2 / 5 × 10ⁿ 中第一个够宽的步长；其间的细刻度取步长的十分之一、五分之一或一半里第一个不小于 6 px 的（帧单位只取整帧）。缩放时刻度自动加密或变稀。
- 在标尺上按下并拖动移动播放头，拖动中发 `TimelinePlayheadChanged { committed: false }`，松手发一次 `committed: true`。开着 `.snap(true)`（默认）时落在整帧上。

## 轨道

- `TimelineTrack::depth(n)` 设嵌套深度：一条轨道属于它前面最近的深度更小的轨道。`.expanded(bool)` 让它成为分支。
- 点分支左边的三角折叠或展开它，被折叠的子轨道不显示，并发 `TimelineTrackToggled { track, expanded }`。
- 轨道和关键点区域一起纵向滚动（`view.scroll`）。

## 关键点

- 点一个关键点选中它，Shift 点选加入或移出选择；在空白处拖出一个框，框里的关键点被选中（Shift 时加入）。选择变化时发 `TimelineSelectionChanged { keys }`，选择保存在 `selection`（按 `TimelineKeyRef { track, key }` 排序）。
- 拖动已选中的关键点移动整个选择。开着吸附时，第一个选中的关键点落在整帧上。拖动中组件自己画出移动后的样子，并发 `TimelineKeysMoved { committed: false }`；松手发一次 `committed: true`，应用这时改数据。
- 删除、全选、适配视图这类按键属于应用的快捷键：应用读 `selection` 删除关键点，调用 `cx.fit_timeline(entity)` 适配视图。

## 曲线视图

- `.mode(TimelineMode::Curves)`：每条可见轨道的关键点用三次贝塞尔曲线相连，控制点是关键点的出切线和下一个关键点的入切线（`TimelineKey::out_handle` / `in_handle`，相对关键点的 `[dt, dv]`）。曲线在首末关键点之外保持水平。
- 选中的关键点显示两个切线手柄。拖动手柄发 `TimelineHandleMoved { key, out, handle, broken }`。手柄保持在关键点的同侧（出切线向后、入切线向前）。`broken` 为假时，另一侧手柄转到相反方向、保持原长度，组件在拖动中这样画，应用提交时也这样改；关键点本身 `broken`，或拖动时按住 Alt，两侧手柄各自独立。
- 曲线视图里拖动关键点同时改时间和值（`dv`）。
- `fit()` / `cx.fit_timeline()` 把选中的关键点（没有选中时是全部关键点）放进视图：时间铺满关键点区域，值和切线手柄铺满高度。

## 视图

- 滚轮以指针处的时间为中心缩放；Shift + 滚轮滚动轨道；Ctrl + 滚轮平移时间。中键拖动平移（曲线视图里同时平移值）。
- 视图改变时发 `TimelineViewChanged { view }`。应用可以保存 `view`，下次用 `.view(...)` 恢复。视图重新渲染时，用户缩放和平移过的视图不会被覆盖。

## 属性

| 属性 | 类型 | 说明 |
| --- | --- | --- |
| `TimelineEditor::new(轨道)` | `TimelineTrack` | 轨道和关键点 |
| `.fps(f64)` | `f64` | 每秒帧数，默认 30 |
| `.unit(TimelineUnit)` | `Frames` / `Seconds` | 标尺单位 |
| `.mode(TimelineMode)` | `DopeSheet` / `Curves` | 关键点视图或曲线视图 |
| `.view(TimelineView)` | `start`, `pixels_per_second`, `value_min`, `value_max`, `scroll` | 可见窗口 |
| `.playhead(f64)` | 秒 | 播放头 |
| `.snap(bool)` | `bool` | 拖动吸附到整帧，默认开 |
| `.header_width(px)` | `f32` | 轨道标题列宽，默认 140 |
| `.label(文字)` | `Arc<str>` | 读屏名称 |

## 事件

| 事件 | 载荷 | 说明 |
| --- | --- | --- |
| `TimelinePlayheadChanged` | `time`, `committed` | 播放头拖动中 / 松手 |
| `TimelineSelectionChanged` | `keys` | 选中的关键点变化 |
| `TimelineKeysMoved` | `keys`, `dt`, `dv`, `committed` | 关键点拖动中 / 松手 |
| `TimelineHandleMoved` | `key`, `out`, `handle`, `broken`, `committed` | 切线手柄拖动中 / 松手 |
| `TimelineTrackToggled` | `track`, `expanded` | 分支折叠或展开 |
| `TimelineViewChanged` | `view` | 缩放、平移或滚动 |

## 参见

[PropertyGrid](property-grid.md) · [总览](index.md)
