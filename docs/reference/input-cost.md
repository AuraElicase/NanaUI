# 单次指针事件的成本与脏帧分析

本文记录 Vue L1/L2 与 Rust L3 在指针高频移动（hover）下的实测开销、四轮架构收敛过程，以及当事件处理器触发真实视图更新时的脏帧成本分布。基准测试源码位于 [`crates/nana-ui-devtools/src/bin/nana-hover-benchmark.rs`](../../crates/nana-ui-devtools/src/bin/nana-hover-benchmark.rs)，原始性能档案见 [`archive/docs-notes/performance-data/vue-hover-2026-09-08/`](../../archive/docs-notes/performance-data/vue-hover-2026-09-08) 与 [`archive/docs-notes/runtime-dirty-frame.md`](../../archive/docs-notes/runtime-dirty-frame.md)。

## 核心结论

1. **指针事件派发本身在两条路径上均为 $O(1)$ 常数开销，不随树规模增长。** 在 250 至 2,000 行的列表上，Rust L3 单次指针移动恒为 **`0.0003 ms`**，Vue 路径恒为 **`~0.060 ms`**（虽有 V8 桥接的固定底噪，但绝对值仍在 `0.1 ms` 以内）。因此，“指针事件密集”不再是排斥 Vue 入口的理由。
2. **唯一会导致耗时随列表长度增长的是「整列重渲染」写法。** 当模板中的 `v-for` 位于同一个渲染函数内，且 `onPointerenter` 修改了参与渲染的响应式状态时，Vue 会对整列执行 vnode diff；将每行拆为独立子组件可省去大头开销，而当局部修改的欠账本身为常数时，Runtime 增量布局与绘制同样保持常数耗时（502 至 8,002 节点均在 `0.008–0.011 ms`）。

## 基准测试设计

测试在同一个进程内依次构建四棵结构相同的树（一列定高 `24 px` 的行，指针逐行下移并回绕），并提供带滚动容器的 `-scroll` 变体：

| 变体名称 | 每行的设置 | 测量目的 |
| --- | --- | --- |
| `bare` | 无任何指针事件监听器 | 测量纯粹的宿主命中测试与帧泵底噪 |
| `listeners` | 每行挂载 `onPointerenter`，仅递增一个不参与渲染的普通计数器 | 验证将事件跨界送入 JS 闭包的真实成本 |
| `reactive` | 每行挂载 `onPointerenter`，修改整列共用的响应式 `ref` 以切换行内联样式 | 模拟单组件 `v-for` 下触发整列重算的最坏写法 |
| `reactive-components` | 可见效果与 `reactive` 完全一致，但每行封装为独立子组件、各自持有 hover `ref` | 测量组件级隔离能省去多少 vnode diff 与同步开销 |
| `runtime-l3` | 纯 Rust L3 保留树节点 | 测量 Rust 原生路径的基准耗时 |

测试支持两种驱动形状（`--shape`）：`window` 运行真实窗口重绘路径 `VueHost::prepare_window_frame`；`headless` 运行 Agent 会话使用的 `flush_scene_frame`。性能结论必须以生产窗口实际执行的 **`window` 形状**为准。

## 实测数据（macOS / Apple Silicon，`--release`，60 次预热 + 400 次移动，P50 毫秒）

### 阶段一：派发路径收敛后的基线（脏帧布局增量优化前）

| 行数 | `bare` | `listeners` | `reactive` | `reactive-components` | Rust L3 |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 250 | 0.056 | 0.057 | 0.78 | 0.61 | 0.0003 |
| 500 | 0.057 | 0.057 | 1.47 | 1.09 | 0.0003 |
| 1,000 | 0.060 | 0.058 | 3.35 | 2.23 | 0.0003 |
| 2,000 | 0.061 | 0.058 | 8.63 | 5.27 | 0.0003 |

### 阶段二：脏帧无障碍种子与兄弟样式重复解析修复后（2,000 行）

在进一步消除「布局脏集向无障碍投影广播至文档根」以及「布局引擎对每个兄弟节点重复解析 effective style」两处 $O(N)$ 瓶颈后，触发真实状态变更的 `reactive` 与 `reactive-components` 进一步大幅下降：

| 2,000 行指标 | 修复前 | 修复后 | 降幅 |
| --- | ---: | ---: | --- |
| `reactive`（每事件总耗时） | 8.87 ms | **4.78 ms** | −46% |
| `reactive`（settle 阶段） | 2.66 ms | **0.73 ms** | 降至约 1/4 |
| `reactive-components`（每事件总耗时） | 5.45 ms | **1.79 ms** | −67% |
| 纯常数局部改动（502 → 8,002 节点） | 随节点数线性增长 | **0.008–0.011 ms** | 收敛为 $O(1)$ |

（注：滚动容器版本与非滚动版本耗时一致，2,000 行 `bare-scroll` 为 `0.065 ms`。）

## 我们是如何把两条路径都收敛到 $O(1)$ 的

### 1. Vue 路径的四轮去线性化

| 改动点 | 解决的根因 | 2,000 行效果 |
| --- | --- | --- |
| `resolve_layout` 按 revision 守卫（`f3ad6d3`） | 此前每事件执行七段全树遍历重算相同的布局结果（注：挂载后需在单次调用内跑到不动点再记状态） | 无头 `1.444 → 0.379 ms` |
| `sync_svg_rasters` 改走索引（`1c13923`） | 此前每窗口帧最多四次全节点扫描 + 每节点一次 `String` 分配 | 生产窗口 `0.583 → 0.062 ms` |
| `patchProp` 的 `style` 按值比较（`ea2f721`） | 内联 `:style` 每次渲染生成新对象引用，导致每行跨界触发一次 CSS 级联 | `reactive` `60.3 → 10.9 ms` |
| `flush_scene_frame` 按 Scene instance 守卫（`5d2015e`） | 此前每帧重录全部可见绘制盒（每节点三次加锁） | 无头 `0.258 → 0.063 ms`；滚动 `1.903 → 0.064 ms` |

此外，JS 侧事件 invoker 在闭包更新时原本已通过 `existing.value = handler` 避免了重绑，但旧代码紧接着仍调用 `hostCall("patchProp", [nid, key, true])` 重复通知宿主入队 world mutation；移除该冗余跨界调用后，2,000 行 `reactive` 再从 `10.9 ms` 降至 `8.97 ms`。

### 2. Rust L3 路径的组件类型索引（`0.0819 ms` → `0.0003 ms`）

早期基准曾把事件时间戳固定传为 `Duration::ZERO`，导致带 `8 ms` 节流的路径只执行第一次，掩盖了 L3 侧随节点数增长的扫描开销（真实 2,000 行耗时为 `0.0819 ms`）。将基准改为每事件推进固定 `16 ms` 步长后，分段计时抓出了四处「通过扫描全文档 `document_order` 查找特定组件」的线性热点：

- `active_runtime_overlays`（改用已有的 `UiWorld::overlay_hosts_by_document` 索引）；
- `split_handle_near`、`sync_split_handle_hover` 与 `clear_calendar_heatmap_hover`。

**根治方案**：在唯一权威 `UiWorld` 内维护 `nodes_by_component: HashMap<ComponentTypeId, HashSet<StableNodeId>>`，于 `SetComponentType` 应用与节点销毁（despawn）时同步更新（不能挂在 `AppContext` 层，否则 `finish_semantic_binding` 语义绑定路径会漏记）。修复后，L3 单次指针事件在任何规模下均稳定在 **`0.0003 ms`**，且 `component-gallery` 的 559 张像素基线全部匹配。

### 3. 脏帧（Dirty Frame）在 Vue 桥接层的剩余线性项

当一次 hover 确实修改了 2 个控件的样式时，对 500 行与 2,000 行两个规模做细粒度阶段计时表明：虽然 Vue 投影的脏控件集合是常数（仅投影发生变化的控件），但 `VueHost::sync_semantics` 的 settle 阶段仍有约 73% 的耗时落在 Vue 桥接层的四处文档级扫描上：`try_bind_registered_component`、`find_sidebar_reparent_host`、`sync_layout_containing_blocks` 以及交给 Runtime 的脏工作同步。这也是后续继续将 Vue 脏帧 settle 压向纯常数的优化靶点（详见 [`runtime-dirty-frame.md`](../../archive/docs-notes/runtime-dirty-frame.md)）。

## 复现命令与等价性门禁

```bash
for mode in bare listeners reactive bare-scroll listeners-scroll reactive-scroll; do
  node crates/nana-js-engine/fixtures/vue-sfc-compat/build-hover-bench.mjs $mode 2000
done
cargo build --release -p nana-ui-devtools --features agent-bin --bin nana-hover-benchmark
./target/release/nana-hover-benchmark --rows 2000 --moves 400 --warmup 60 --shape window
./target/release/nana-hover-benchmark --rows 2000 --moves 400 --warmup 60 --shape headless --scroll
```

所有「按修订号/实例号跳过重算」的快路径门禁，均由 `crates/nana-ui-vue/src/host/frame.rs` 中的**双宿主等价性测试**（开启快路径门 vs 关闭快路径门，逐步比对文档快照与 JS 实际读回的绘制几何）在每次 `cargo test --workspace` 中自动守卫。
