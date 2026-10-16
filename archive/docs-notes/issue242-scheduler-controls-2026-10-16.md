# Issue #242 / #354 调度与 direct p50 受控调查（R6）

本轮未确认产品代码根因，尚无可宣称修复的产品补丁。可移植统计及本地证据 SHA 见[结果摘要](performance-data/issue242-r6-2026-10-16/portable-results-v1.json)；原始大文件保存在工作区。当前分支 main，未创建分支或 worktree。新的调查资料全部使用独立前缀保存，不覆盖此前失败、测量或 review。

## 已完成：历史二进制的五组对照

命令：

```sh
python3 -u archive/docs-notes/performance-data/issue242-r6-2026-10-16/historical/run_historical.py --prefix r6-historical-control-20261016-1
```

冻结的 R1 (`da2bb7a0…563d`) 与 R2 (`793f7a55…4de4`) 使用相同实际 executable 路径、相同最小 supplied environment、cwd `/`、原始完整六阶段 CLI。每进程预热 30、测量 240 帧，顺序 ABBA / BAAB / ABBA / BAAB / ABBA，共 20 次实际窗口运行，无 helper 或 trace。

| block | 顺序 | R2−R1 p50 配对均值 (ms) |
|---|---|---:|
| 1 | ABBA | +0.0016875 |
| 2 | BAAB | −0.3252090 |
| 3 | ABBA | −0.3046460 |
| 4 | BAAB | −0.0708135 |
| 5 | ABBA | −0.3927705 |

五组差值中位数 −0.304646 ms。20 次全部通过现有窗口 probe 的六阶段功能断言、真实 GPU 工作量、painter/heap 指标、输入哈希与前后会话门禁。独立 reviewer 重新验证了 60 份原始 report/stdout/stderr 的哈希/大小、运行次序和组间统计，记录在 `performance-data/issue242-r6-2026-10-16/review/historical-actual-control-review.json`。

这个新 population 没有稳定复现先前正向版本差值；同一 R2 的 block 2 p50 可由 0.244333 变为 1.098833 ms。**这不是产品优化证据，也不是没有上下文相关回归的证明。** 新的最小环境/公共 bundle 与旧测量不同，实际最终环境、逐帧物理尺寸/present mode、P/E/TG、连续前台和 thermal 状态均不可从历史二进制补出。中途系统负载快照仅为补充观察，不代表隔离；这轮未增加逐像素视觉验收。

## 启动与调度 trace

同一冻结 `5461e570…91a9` 二进制与 constructor-only `bf58ea1d…f9dd9` observer，供应环境/cwd 固定，实际环境单独校验。原始 36 秒四进程采集因保存容量门禁失败，未计为有效结果。V5 的前置空间失败在启动目标前发生。

V6 在第一个有效分段之前冻结新 population：240 帧预热、600 帧测量，每段两进程/固定 20 秒自然停录；十段组成预注册的五组 ABBA/BAAB 顺序。分段会改变跟踪器生命周期与跨段 bundle 路径，按 block 比较，保留位置与跨段漂移，不与旧 36 秒或 smoke 样本合并。任一半段缺失或门禁失败，该 block 不通过。actual trace 覆盖、独立 review 未完成前 collector 的 `block-complete-awaiting-exact-trace-and-independent-review` 不代表验收通过。

启动路径会同时改变父进程、TG/coalition、AppKit/环境等。还必须检查观察者/tracer 的真实执行 TG：不能按父 PID 推断共享，也不能把 trace 本身可能施加的同组负载当作产品根因。运行系统的 XNU 与公开源码标签不一致，未建立准确 ABI 的字段保持 raw/unknown。

## V6 全量功能与逐帧验收

十段均以 xctrace exit 0 自然结束，没有重排或替换其中的样本。原始失败单列，不计入五组。20 个目标的 80 份原始产物、全部输入前后哈希、上下文、逐层 counter、实际环境和输出工作量已独立审查。每个目标的 600 个测量帧恰有 600 次 render/submit、1,800 次 draw、807,096 bytes upload，copy/readback/extra-pass/plan-rebuild/recreate 为零；Rust heap 七项均真实观测。

独立审计没有导入生产 decoder/calibrator，而是从 XML 的 uint64 与 Fraction 时钟区间重算：全部 16,800 帧 bounds/preferences 完整覆盖，12,000 个测量帧无 excluded。实际 anchors 为每进程 8 个、每 pair 16 个，共 160 个；按实际数组逐项匹配，不由固定数补齐。审查收据逐段绑定 state、analysis、frames、anchors、三个 XML 的 SHA；清单为 `performance-data/issue242-r6-2026-10-16/review/v6-five-block-independent-review-manifest.json`。

在 6,000 个 measured direct 帧中，P95/E95/mixed 为 3,874/2,100/26；6,000 个 measured LaunchServices 帧均为 E95。P95/E95 指观测性能层级 ticks 中对应层级占比至少 95%，不是核心绑核或频率证明。第一段 LS 捕获五个符合冻结 CPU/E95 阈值的连续慢段，长度 64/61/173/72/95 帧；最长 presented 397–569。该同二进制实验表明 route 伴随执行归属变化；它不是 TG 唯一因果、历史 R1/R2 根因或新 main 二进制的时延验收。

五组统计现已从 20 份原始 samples 独立重算。按原 report 的 lower-order p50 定义（600 样本取 index 299），每组 LS−direct 配对均值为：

| block | wall p50 差值 (ms) |
|---|---:|
| 1 | +0.4781880 |
| 2 | +0.3331880 |
| 3 | +0.5293335 |
| 4 | +0.5437500 |
| 5 | +0.2581670 |

五组平均 +0.4285253 ms；execution observer 线程 CPU p50 差值平均 +0.4241289 ms。benchmark 自身的 `samples.thread_cpu_ms` 与 `execution.observation.thread_cpu_ns` 观测区间略异，分别报告，不能混为一个字段。GPU timestamp 没有采集，不把 wall 与 CPU 的差值直接解释为 GPU 等待。direct 同臂跨段 wall p50 漂移仍可达 −0.803833 / +0.744416 ms；同一 TG 数字在不同 capture 中不作为跨 capture identity 证明。

所有 measured targetPID 的已记录 raw policy 与已配对 packed 改变均为 0；direct 六个 process 有 114–534 次推荐 tuple 改变，其他 process 为 0。**本次测量区间的推荐变化未与已采 target raw policy 记录共现**，不证明真实 policy 从未改变，也未识别私有控制器上游触发。首段 xctrace launcher PID 未取得无歧义 birth/initial-SET 链，其执行 TG 保持 unknown；不能以缺失链推断它没有与目标共享 TG。五组结果为稳定同符号的启动总效应关联，不是 TG 唯一因果或历史二进制回归的产品修复。

统计独立记录：`performance-data/issue242-r6-2026-10-16/review/review_v6_five_block_statistics_v2.json`；完整结果：`performance-data/issue242-r6-2026-10-16/analysis/v6-five-block-summary-v2.json`。实际证据清单 `analysis/v6-series-actual-artifact-manifest.json` 包含 2,103 个已保存文件、3,062,570,622 bytes 的 SHA/大小，完整原始 trace 均在本地保留。

补充的 post-hoc 分层也已直接从 20 份原始 execution level deltas 独立复算（12,000 帧、300 个指标格）：同一 direct 进程中，P95 帧 observer CPU p50 约 0.213–0.260 ms，E95 帧约 1.079–1.190 ms，退休指令数约 1.026–1.065M。层内帧不作为独立随机重复；空层明确 null/count 0，没有归一化或改写正式分数。全 P95 的 direct 进程仍有约 0.293–0.400 ms 的 p50 漂移，故 P/E 分类也不足以解释所有时延变化。结果为 `analysis/v6-process-attribution-strata-v2.json`，actual 独审为 `review/review_process_strata_actual.json`。

首段 global policy 补充独审仅证明：Codex PID 59862 的 14 个 selected threads 曾在同一 capture 使用 direct 的 TG 数字。44 条该 PID 的原始 policy 记录与 direct 全生命区间 564 次推荐 tuple 改变，在预先规定的 ±2ms + 完整 clock uncertainty + 2ns 窗内未匹配。measured 子区间有 516 次改变；LS 全生命区间有 72 次改变，但没有符合所选 ever-match actor 的关联。未知 parent/tracer 链、其他 actor、未采事件与实际私有 ABI 仍未知，不能由 0 共时否定观察者工作负载反馈。独审为 `review/review_related_policy_actual_v2.json`。

当前主解析、collectors、时钟校验与 pair-aware 导出均有 CPU 行为测试与独立 review。汇总工具旧草稿在运行前的独审中发现三处问题：CPU 字段取错层级、p50 lower-order statistic 被误写成两中值平均、block 未严格核对冻结的 pair 顺序。原始测量和主逐帧分析不受影响；修复后的 V2 已通过真实首段贯通、错序等 9 项 CPU 测试和独立统计复算，旧草稿与失败记录保留。

实际重跑：`cargo fmt --all -- --check`、`git diff --check`、engine-boundary、api-convergence、perf schema、perf self-test 均 exit 0。记录见 `performance-data/issue242-r6-2026-10-16/validation/root-post-capture-checks.json`。本轮没有改动框架 Rust 产品源代码，未把既往 workspace 测试冒充本轮重新运行。上述六项仓库检查基点为 `806549d69`；发布报告时已保留并 rebase 到远端 `5cd2b70c9`，其新增渲染/toolbar 改动未纳入冻结二进制性能数据。

## 证据与工具故障

原始失败 trace 使用 BSD tar 保存权限、owner、纳秒 mtime、xattr、ACL、flags，经原始字节 SHA 与独立完整还原验证后，仅回收重复展开副本；压缩原始字节与旧失败 state/log 均保留。仅清理逐文件哈希、stat、无打开句柄及独立审查确认的可再生已完成测试缓存，不删除源码、冻结性能二进制、helper 或无关工作。

首个 V6 pair 的 xctrace 报告 Cannot save trace document，并出现 Instruments Packages NSDistributedLock 异常。原 state 未能完成 finalization，不予改写为通过；8 份 owned temp 报表另存 forensic receipt。期间可用空间由约 0.395 GB 变为约 85 GB，外部变化来源未知，本代理未执行全局数据删除。随后重测使用新前缀。

## 代码缺口与验证缺口

- 代码根因：尚未确认。没有采用绑核、修改 QoS 或恢复无调用方抽象来掩盖信号。
- 诊断代码/观测缺口：固定 target 的双 resident worker 负载实验尚缺完整正式运行编排；旧冻结 fixture 没有持续物理尺寸与实际 present mode，最小 worker 没有退休指令 getter。这些不是平台“不支持”的证明。
- 因果证据缺口：持续慢状态已捕获，推荐变化的上游触发仍未确认；启动总效应与 TG/观察者竞争负载尚未分离。
- 平台/源码验证缺口：运行 XNU13432 的 packed ABI 未取得匹配源码；Windows/native/SteamVR/Electron/device loss 的真实平台矩阵未补测。
- 已修复的调查工具问题（非产品补丁）：C JSON 整数布尔解析、模糊 policy START/END 拒绝跨调用配对、容量门禁/失败保留，以及真实 V6 pair population 的导出准入。
- #354 保持未关闭；未执行的平台测试不列为通过，不自动关闭 #242。
