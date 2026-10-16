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

## 固定 target 的双 worker 控制

本节的[可移植结果与证据 SHA](performance-data/issue242-r6-2026-10-16/portable-worker-results-v1.json)单独记录各 population，不与启动路径对照合并。

无负载身份 pilot 已独立核对实际 NEWTHREAD/initial-SET/SET、公开 PID incarnation、270/240 帧与时钟：该 capture 内 target 与 D worker 使用相同的源码限定 TG，I worker 独立。身份不能沿用到下一次 capture。记录为 `review/worker-identity-actual-trace-review.json`。

正式编排首次请求 3,000 测量帧，但冻结 `5461` 仅支持 240–2,400，目标在 CLI 校验时退出，未准入负载。这是诊断代码兼容性缺陷。修订绑定同一二进制此前 240/2,400 的成功原始记录和此次 3,000 的失败记录；另补录检查双方 ACK 时的实际 MAT，拒绝未来时间的 ACK。V4 独审 21 项编排及 4 项 CLI 测试通过，旧失败与精确源文件备份保留。

20ms 周期 population `r6-worker-formal-20261016-2` 在第二轮失败后停止，后三轮没有启动，不补位。两次目标各自完成 240 预热、2,400 测量，trace 自然 exit 0；每个目标 2,400 render/submit、7,200 draw、3,226,617 bytes upload，七项 heap 与实际上下文均已独审。第二轮 I worker 第 0 期只完成 63,919/65,536 单元：周期实际开始迟到 10.057833ms，剩余 9.942167ms，实际 work wall 9.943917ms，超期 1.75µs。完整 3,200 行的 checksum（含 partial prefix）、CPU、时序和标志均核对，这里的迟到是 `period.start − scheduled`，包含边界等待和 POI 等开销，未独立定位内核唤醒机制；唯一失败不能因处于 settling 而删除。记录为 `review/formal-population2-worker-deadline-audit.json`、`review/review_formal20_raw_functional.json`。

该组第一轮仅为探索结果：独审从原始 XML/Fraction 重算 2,640/2,400 帧、23 个 target anchors、20 个 worker POI 和 2,399 个 gap；固定 16 秒内 target/D 为 TG16455，I 为 TG16477。A 为 D busy/I idle，B 为 I busy/D idle。ABBA 四 steady phase 的 wall p50 为 0.195792 / 0.344166 / 0.817750 / 0.201417ms，样本数 421/420/420/420；全部 settling、边界和窗口外帧保留。**单次反转不是五轮验收，也不证明 TG 唯一因果。** D/I 实际 role 为 0/6，分配负载同时涉及两者固定的启动/环境上下文；worker 未采退休指令，CPU 时间不等于工作单元数。独审为 `review/formal-population2-run1-actual-trace-review.json`。

新 50ms population 独立冻结：依据全部 worker 的最大周期实际开始迟到 10.096ms 和最大 busy wall 13.032708ms，选取能整除 4s phase 与 500ms settling、且不少于两者之和两倍（46.257417ms）的最小支持周期 50ms。K=65,536、T0 lead、16s、target 和 trace 不变，每 phase 80 期、每 worker 320 行；引入工作率为旧协议的 40%，不能混入原组。该选择仅依据已观察容量，不保证未来期限；自动生成的第一轮效果分析当时未向方案制定者读出，但不宣称完全盲态预注册。V5 已通过 22 项编排与 4 项 CLI 独审，34 个冻结文件校验一致，freeze SHA `0648b106d50dade78d76ac3909346f92275410885325a346f44903dcf7de002b`。

`r6-worker-formal-20261016-3` 在会话 preflight 观察到 `com.apple.loginwindow` 后拒绝启动：0 trace/worker/target/plan/样本。该 warning 不作为独立锁屏证明，原失败保留，等待桌面会话恢复后使用同一 V5 定义和新前缀测量。记录为 `review/formal-population3-preflight-review.json`。该 V5 population 的五轮未完成；产品根因修复仍未确认。

50ms 的 `-4` population 第一轮仅取得独审探索结果：2,640/2,400 帧完整，steady phase 样本数 420/420/419/420，wall p50 为 0.210458 / 0.311792 / 0.261708 / 0.240708ms；第二轮从首个 warm 帧就未激活，先前台 Codex、后 Terminal，因此整组停止。用户确认保持测试窗口在前台后，独立新前缀 `-5` 首轮仍从第一帧未激活；没有继续盲目重测。两次失败均保留，不能归责为用户在测量中切走窗口，也不能将 `-4` 的首轮补入新五轮。

已定位并修复的诊断缺陷是**编排要求前台，但 passive 启动路径没有建立实际前台状态**。V6 增加启动阶段的公开 `NSRunningApplication` 单次 activation request，再以实际 frontmost PID、isActive 与公开 incarnation 验证；helper 在 plan/T0 前完成并回收，不在目标逐帧热路径内加调用。请求返回值不算成功，实际 measured context 仍逐帧严格检查。没有修改 QoS、绑核、私有调度策略或产品代码。这是新的启动干预 population，不能直接证明旧 direct p50 回归已修复。

首个 cold-front pilot 的窗口、2,400 次 GPU render/submit 和 640 个 worker 计算周期通过，但 xctrace 在旧等待期限内没有完成保存，最终 trace 不予验收。旧代码的 90s 等待从 ready 后约 24s 开始，包含剩余录制，给录制后处理的时间更短。V7 把终止期限明确为 ready + 45s 录制 + 180s 后处理；必要时只在测量结束后对本任务尚未回收的 tracer 采样一次。录制长度与计算量不变。独立定义审查通过 54 项测试，新 reader 的 40 项测试通过。

V7 pilot `r6-worker-coldfront-pilot-20261016-2` 已完成独立全量验收：19 份原始产物、83 项前后输入 SHA、全部 2,640/2,400 帧和 640 个完整 checksum/CPU/deadline 行、23 个 target anchors、20 个 worker POI、2,399 个 gap。该 capture 的 target/D 同属源码限定 TG16455，I 为 TG16483；跨 capture 不沿用数字身份。tracer 自然 exit 0，发生在 ready 后 152.522804s，即名义录制结束后 107.522804s；实际测后单次堆栈显示当时位于 symbolication/shared-cache 路径。这只解释采样那一秒的后处理位置，不能证明全部延迟原因，更不是产品帧慢的堆栈。该 pilot 明确排除在正式五轮之外，收据为 `review/coldfront-v7-pilot-complete-independent-review.json`。

以每个 phase 原始 lower-order p50 为基础，每轮分别平均两个 A 与两个 B phase，再对五轮等权汇总。A 为 D busy/I idle，B 为 I busy/D idle，计算单元及总期数固定。

| run | order | B−A wall p50 phase 均值 (ms) |
|---|---|---:|
| 1 | ABBA | +0.1875840 |
| 2 | BAAB | +0.1895835 |
| 3 | ABBA | +0.3081875 |
| 4 | BAAB | +0.2458540 |
| 5 | ABBA | +0.1456455 |

五轮平均 +0.2153709ms；observer execution CPU 平均差 +207,504.3ns，benchmark 自身 thread CPU 平均差 +0.207575ms，两个区间保持分别报告。原始五个指标、20 phase、同臂漂移、推荐 tuple 全时间线和 clock uncertainty 均已独立重算。12,000 个 measured 帧的 P95/E95/mixed 为 6,554/5,370/76。按冻结的连续至少 60 帧、E95 且 execution CPU ≥900,000ns 条件，本组连续慢段为 0；不等于没有单帧慢样本，也不覆盖旧 population 的慢段。

该结果将可重复信号定位到**固定 resident 的计算负载分配输入**：仅切换哪个现有进程做相同工作，五轮差值同号。D/I role0/6、启动与实际环境交互仍属于处理的一部分，不能把它缩写成“TG 负载唯一因果”，也不能据此修改产品 QoS 或重录时延基线。同臂漂移与外部负载仍保留。实际汇总为 `analysis/formal-v7-five-descriptive-summary-v2.json`，统计独审为 `review/review_formal_v7_five_statistics.json`，逐轮收据清单为 `review/formal-v7-five-actual-acceptance-manifest.json`。

另修正 worker marker 时钟校验：合法的相邻采样 bracket 可以共享一个 MAT 端点；仅 worker 校准器允许端点相接，继续拒绝真实重叠、非递增 POI、重复 marker 与 epoch 错误。旧 target 校准逻辑未改，原始输入未改写。此缺陷影响旧 reader 的准入，不是帧时延回归。

V7 冻结 SHA 为 `bbd48ba099434e255f560d2dd59c436a542be44791cdcdd0df67789c9277d6c9`，47 个 payload 精确校验。完整五轮仍使用 ABBA/BAAB/ABBA/BAAB/ABBA、每 phase 4s、50ms 周期、500ms settling、每 busy 期 65,536 单元；每轮必须重新审查身份与原始 trace。五轮所需留存空间先通过无损归档和独立完整恢复验证解决，12 GiB 总量门禁不放宽，不通过移动计量目录规避。正式五轮 `r6-worker-coldfront-formal-20261016-1` 已全部自然保存 exit 0，没有替换或重排样本。原始功能独审通过 95 份 raw、5×83 项输入前后哈希、13,200 帧与 12,000 测量帧、24,000 个严格 measured context 端点，以及 5×640 个完整 worker 周期。每轮均为 2,400 render/submit、7,200 draw、3,226,617 bytes upload 和七项实际 heap 观测。资源采样记录到的最大留存为 11,474,976,384 bytes，低于原 12 GiB 门禁。全部逐帧 XML/Fraction、身份、推荐时间线及五轮统计现已通过独立验收：共 115 个 target anchors、100 个 worker POI、11,995 个 gap，15 个 selected process unique ID 均独立，无 excluded。该验收仅接受本 population 的描述性证据，尚无产品根因修复结论。

本节草稿后与五轮测后均实际重跑 fmt、diff、engine-boundary、api-convergence、perf schema/self-test 六项均 exit 0，分别记录为 `validation/worker-report-checks.json`、`validation/worker-final-checks.json`（基点 `01078fc03`）。本节未修改框架 Rust 源码，不把诊断工具测试或本 population 窗口证据计作 workspace/Clippy、当前 main 时延验收或平台矩阵通过。

## 证据与工具故障

原始失败 trace 使用 BSD tar 保存权限、owner、纳秒 mtime、xattr、ACL、flags，经原始字节 SHA 与独立完整还原验证后，仅回收重复展开副本；压缩原始字节与旧失败 state/log 均保留。 本轮另对已完成的 `control/runs`、旧 `analysis/runs` 与失败 V6 partial trace 共 3,184 项作相同完整验证，三份无损档案实际节省 3,078,469,748 bytes。旧清单和 state 中的路径不改写，恢复位置与命令见各自 `retained-*-locator.json` 及 failed-pilot locator；当前 V7 输入和新分析不在这些目录。正式五轮启动前按实际占用复算，包含完整 1 GiB 导出/review 预留的预测峰为 12,717,750,754 bytes，低于原 12 GiB 门禁 167,151,134 bytes；这是单次 pilot 的体积预测，实际采集仍逐次执行原硬门禁。仅清理逐文件哈希、stat、无打开句柄及独立审查确认的可再生已完成测试缓存，不删除源码、冻结性能二进制、helper 或无关工作。

首个 V6 pair 的 xctrace 报告 Cannot save trace document，并出现 Instruments Packages NSDistributedLock 异常。原 state 未能完成 finalization，不予改写为通过；8 份 owned temp 报表另存 forensic receipt。期间可用空间由约 0.395 GB 变为约 85 GB，外部变化来源未知，本代理未执行全局数据删除。随后重测使用新前缀。

## 代码缺口与验证缺口

- 代码根因：尚未确认。没有采用绑核、修改 QoS 或恢复无调用方抽象来掩盖信号。
- 诊断代码/观测缺口：双 resident worker 编排已实现并独审，20ms 五轮因实际 deadline 失败未完成，50ms 的前台失败与 tracer 后处理等待缺陷已修订，V7 pilot 已独审，正式五轮已采集且原始功能独审通过，逐帧 XML 与统计独审均已完成；固定 role/启动上下文尚未分离；旧冻结 fixture 没有持续物理尺寸与实际 present mode，最小 worker 没有退休指令 getter。这些不是平台“不支持”的证明。
- 因果证据缺口：持续慢状态已捕获，推荐变化的上游触发仍未确认；启动总效应与 TG/观察者竞争负载尚未分离。 固定 worker 负载分配仍包含 D role0 / I role6 的启动上下文交互，不等同于 TG 唯一干预；plan 原子发布时间未直接存 MAT，LS worker 非子进程的退出也不能由 launcher exit 0 代替。
- 平台/源码验证缺口：运行 XNU13432 的 packed ABI 未取得匹配源码；Windows/native/SteamVR/Electron/device loss 的真实平台矩阵未补测。
- 已修复的调查工具问题（非产品补丁）：C JSON 整数布尔解析、模糊 policy START/END 拒绝跨调用配对、容量门禁/失败保留，以及真实 V6 pair population 的导出准入。
- #354 保持未关闭；未执行的平台测试不列为通过，不自动关闭 #242。
