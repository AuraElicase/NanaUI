# Issue #357 收尾记录（2026-10-21）

headless `PresentationDriver` 的不兼容输出组共用一个 `SceneWgpuPainter`，这部分由 286ff2598 实现。本记录是它的后续：同帧多组录制的安全、资源计数、测试，以及五轮 Release 对照。仅 Metal（Apple M 系列，本机）。

## 发现并修复：同帧多目标的字形图集覆写

- 原因：一个 `FrameContext` 的上传批在提交时整体排在该帧所有 pass 之前，而字形图集被 painter 绘制的所有目标共享，`atlas.begin_frame` 又是按 paint 推进的。后一组可以淘汰或 compact 前一组的字形，它的上传落地后，前一组已录制的 pass 就会采样到后一组的字形。`placement_epoch` 只能修正下一帧。
- 影响范围：headless 不兼容组；窗口与同 profile 输出共用 painter 的同一帧。
- 修复：同一帧内第二个及之后的 paint，把字形区域改为在 encoder 的录制位置上 `copy_buffer_to_texture`。暂存缓冲按 256 字节行对齐，并按 `max_buffer_size` 分块。首个 paint 仍走上传批。
- 丢帧处理：这些拷贝随 encoder 一起丢失，所以记为 painter 的 `RetainedWrites`。帧被丢弃时，下一次 `begin_frame` 让图集忘掉全部位置；placement epoch 前移，各目标重新放置并重建批次。
- 合同：一个 painter 同一时刻只向一个帧录制，见 `paint_target` 文档与 `docs/reference/gpu.md`。乱序提交两个并存帧的情况没有强制检查，仓库内没有这种路径。

## 资源归属与计数

| 资源 | 归属 | 计数 |
| --- | --- | --- |
| 文本引擎、shape / raster 缓存、字形图集 | painter 共享 | `text_glyph_counters()` |
| `UrlTextureCache` | painter 共享，按 egress 与源编码分桶 | `resource_counters()`：`url_textures` / `url_texture_gpu_bytes` / `url_texture_cpu_bytes` / `url_pending` |
| motion 资源 | painter 共享 | — |
| 图标图集 | 每个画过图标的 `RenderTargetId` 一份（painter 默认目标同样），第一个图标出现时才创建 | `resource_counters()`：`icon_atlases` / `icon_atlas_bytes` |
| 管线集 | 每个工作格式一份，移除 profile 时回收 | — |
| 目标状态（uniform、实例、dest） | 每目标 | — |

`PresentationDriver::text_glyph_counters()` 与 `painter_resource_counters()` 读取共享 painter 的数字。计的是纹素字节，不含驱动填充，也不是显存。

`output-incompatible` 的数字取自 B 的 5 轮，每轮相同。场景是 SDR RGBA8 sRGB 与其 force-float working 两组，128×96，一个文本节点：

- 字形图集：3 页（含两个 1×1 占位页），1,048,581 B；
- raster 缓存：1,082 B；字形上传：1,796 B；
- 图标图集：6 个，1,572,864 B（默认 1 个，加上两组共 5 个已绘制槽位，每个 256²×4）；
- URL 纹理：0。

上面图标图集的数字是这次提交时的状态，当时图集在目标创建时就分配，场景里没有图标也照样占用。后续改成惰性分配后，同一 workload 报告 `icon_atlases: 0`、`icon_atlas_bytes: 0`，测试是 `shared_painter::icon_atlases_are_allocated_by_the_first_icon_a_target_draws`。A 的这两类资源数字不可得：A 没有这些计数器。字形图集方面，286ff2598 的测试记录了两组从 1 MiB 变成 2 MiB；图标与 URL 的 A 侧成本按 unknown 记，不由结构推算。

## 测试

- `scene_paint::tests::a_later_target_in_one_frame_cannot_overwrite_glyphs_an_earlier_one_samples`
- `output::driver_tests::shared_painter`，共六项：同帧淘汰、三组参数快照、静态复用、detach/resize/reattach、设备代际与丢失、两组同帧丢弃回滚。
- `url_texture_cache::tests::each_source_encoding_keeps_its_own_texture` 增加了计数断言。

变异实验：
- 关掉按序上传，两个图集测试变红；
- 关掉丢帧回滚，丢帧测试变红；
- 恢复后均为绿。

## 五轮 Release 对照

- A：HEAD 84599a4db 反向应用 286ff2598，也就是每个 profile 一个 painter。
- B：HEAD 加本次改动；其他会话的未提交改动不在内。
- 两棵树都是导出树，用各自的 `CARGO_TARGET_DIR`，带同一份 `incompatible` workload 补丁（A 侧去掉计数器调用）。
- Window probe 用两侧相同的 HEAD 源码，只用默认 flag。
- 每个 variant 先跑 headless benchmark，再跑 Window，按 AB/BA/AB/BA/AB 交替五轮，全程 `caffeinate -d`，1 分钟负载 6.3–7.8。

| 二进制 | SHA256 |
| --- | --- |
| A benchmark | `f326711eac9ae5f173d0eac48218647f6414f6600e8b5a52e6e80215c756e124` |
| B benchmark | `be40f8aa80c159bb3220586991a5d66f671927082c7e72e39f3f8098b7534e66` |
| A Window probe | `38622a329ce7f4147f01598f0bca7e72c305584c1d88af7b4d40728989035387` |
| B Window probe | `308468ec199947c6b3f2e430c6c6abf940f79dcec128458d3b00fa73bffe93f8` |

B 的二进制构建于独立 review 之前。之后的修正只涉及两处：同帧跟随 paint 放置新字形，以及丢帧回滚。两个测量窗口都没有走到这些路径，所以计数不变，但计时没有用最终二进制重跑。

工作量（每轮相同，A = B）：

- headless 全部七个 workload：ui_renders、submissions、extra passes、GPU copies、测量窗口上传字节 / 写入 / 拷贝、painter draw 与上传完全一致。
- `incompatible`：480 render、240 submit、0 extra pass、0 copy、0 上传。Rust 分配次数都是 18,000；分配字节 A 为 3,089,760，B 为 3,024,480。
- Window direct：两侧都通过六阶段验收。240 render/submit、720 draw/batch、323,473 上传字节、0 extra pass/copy，分配 60,850 次。不兼容阶段为 120 render、60 submit。

CPU 调用墙钟，单位 ms，不是 GPU 时长：

| 场景 | A 五轮 p50 | B 五轮 p50 | min A / B |
| --- | --- | --- | --- |
| Window direct | 0.437 / 0.254 / 0.246 / 0.236 / 0.290 | 0.386 / 0.420 / 0.217 / 0.218 / 0.231 | 0.236 / 0.217 |
| headless incompatible | 0.0259 / 0.0254 / 0.0774 / 0.0245 / 0.0240 | 0.0345 / 0.189 / 0.0245 / 0.0330 / 0.0240 | 0.0240 / 0.0240 |

- Window direct：逐轮差值四降一升，min 没有退化。这不作为时延改善的结论；direct 时延的归因由 #354 跟踪。
- headless incompatible：min 相同，中位 B 高约 8 µs，三轮偏高、两轮持平。这属于单进程噪声量级，没有证明退化，也没有证明无差。
- GPU duration 与 driver VRAM：unknown。

## 验证命令

最终提交重放到 origin/main `aaae8bc77` 之上，在只含本次改动的导出树上：

- `WGPU_BACKEND=metal cargo test --locked --release -p nana-ui --features hosted,bundled-fonts --lib`：955 passed / 1 ignored。
- `cargo test -p component-gallery --bin ui-snapshots --features snapshots`：12/12。
- `cargo test -p nana-ui-css --lib`：356/356。
- `cargo fmt --all -- --check`、两个边界脚本、perf `--check-schema` / `--self-test`、7 个 headless 门禁：通过。

在 HEAD `84599a4db` 上开发期间的结果如下（下面两项既有失败已由上游 b2a40396b 与 d21ce0b09 修复）：

- `WGPU_BACKEND=metal cargo test --locked --release -p nana-ui --features hosted,bundled-fonts --lib`：938 passed / 1 ignored，review 修正后连续两次。
- `WGPU_BACKEND=metal cargo test --locked -p nana-ui --features hosted,gpu --lib -- output:: scene_paint::`：578 passed / 1 ignored。
- `WGPU_BACKEND=metal cargo test --locked --workspace --no-fail-fast`：6,255 passed、2 failed、29 ignored。两项失败如下：
  - `nana-ui-css` 的 `every_field_of_the_witness_differs_from_the_default`：单包同样失败；本次没有改动该 crate 及其依赖，属于 HEAD 已有问题。
  - `frame_binding::tests::frames_swap_only_after_present_and_unusable_frames_fall_back_to_the_placeholder`：在工作区负载下 `pool_full` 未收敛；单独重跑 3/3 通过，两次 release 全量也都通过。
- `cargo test -p component-gallery --bin ui-snapshots --features snapshots`：11 passed、1 failed。失败的是 app-menu-bar 两个语义快照；不含本次改动的 A 树结果相同，属于 HEAD 已有问题，没有 bless。
- `cargo fmt --all -- --check`、`git diff --check`、`python3 scripts/check-engine-boundary.py`、`python3 scripts/check-api-convergence.py`、`python3 perf/contract.py --check-schema`、`python3 perf/contract.py --self-test`：均通过。
- 7 个 headless 门禁对真实报告给出 `--evaluate-invariants` = ok。output 门禁共 8 个，另一个 `output-direct` 来自 Window probe。

## 未覆盖与遗留

- 未覆盖 Vulkan、DX12，以及 Windows、Linux 窗口。
- ~~图标图集在每个目标上提前分配，无图标的目标也占用 256 KiB。~~ 已改为第一个图标出现时才分配。
- ~~motion 描述符和时间在 painter 共享，并经上传批写入。窗口与同 profile 的 Separate 输出在一帧内绘制不同场景时，最后写入者获胜。~~ 已通过同帧后续 paint 的按序上传与丢帧缓存回滚修复；keyframe、描述符和时钟的像素回归及验证见 [#242 收尾记录](issue242-closeout-2026-10-26.md)。
