# 平滑滚轮采样开销（2026-10-16）

基线为 NanaUI `83052e4dc6bff9c2d41bb45ff30c5e398845092d`；候选只改变无监听器滚动事件事务的跳过路径、复用待采样 ID 列表。180ms 曲线、8ms deadline、滚动距离和 GPU 路径保持一致。

macOS / Apple Silicon，本地同一 `test` 未优化构建，关闭调试符号与 incremental，计时期间启用线程内分配计数（计数器本身也有成本）。命令及工作负载见 [性能说明](../../perf/README.md#平滑滚轮-cpu-采样诊断)。两个二进制按 ABBA BAAB 交替运行，各四次；每次每种组合预热32次，测量512次过渡，均完成11776个有效采样。表中时间为四轮各自中位数的中位数，分配含重分配。

| 监听器 | 滚动条 | 基线 µs/过渡 | 优化 µs/过渡 | CPU 时间变化 | 分配次数变化（512次过渡） |
| --- | --- | ---: | ---: | ---: | ---: |
| 无 | 隐藏 | 308.50 | 215.94 | -30.0% | 353284 → 294404 (-16.7%) |
| 无 | 自动隐藏 | 317.19 | 223.81 | -29.4% | 353284 → 294404 (-16.7%) |
| 有 | 隐藏 | 315.13 | 315.98 | +0.3% | 353284 → 341508 (-3.3%) |
| 有 | 自动隐藏 | 323.88 | 325.00 | +0.3% | 353284 → 341508 (-3.3%) |

有监听器的 CPU 时间没有可辨认的改善，四轮存在波动，保留这两行的微小回退而不宣称所有滚动都更快。原始逐轮汇总（含 p95）见 [comparison.json](performance-data/smooth-scroll-2026-10-16/comparison.json)。

该诊断只覆盖两节点 ScrollView 的动画查询、采样、事件及 system-work drain，不含起始输入、命中索引更新、实际布局、虚拟行挂载、场景抽取和 GPU 呈现。不能用这些调试构建数字推断发布版窗口帧率、功耗或触控板手感；未测 Windows/Linux。本轮没有修改宿主刷新调度或迁移到 GPU 自主滚动。

验证使用相同的 `CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0` 环境：`cargo test -p nana-ui-runtime --lib --features components,view-macro --locked`（2237通过、1个原有计时测试忽略）；`cargo test -p nana-ui-scene --lib --locked`（227通过）；`cargo clippy -p nana-ui-runtime --lib --test smooth_scroll_cost --features components,view-macro --locked --no-deps -- -D warnings`；`cargo test -p nana-package -p nana-packager --locked`（72通过）。格式、引擎和打包边界脚本通过；debug package-fixture 的打包、外部目录启动与 tamper-suite 通过。夹具的原生 splash、UI-ready、installer 和 static-app-plan 按既有能力标记未执行。本轮未重跑原生窗口或物理触控板验收。
