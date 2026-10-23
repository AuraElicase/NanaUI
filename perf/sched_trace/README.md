# 调度 trace 的记录器事件留存（Issue #359）

[#354](https://github.com/sena-nana/NanaUI/issues/354) 的调度研究 provider 没有采集 kdebug
class 7（`DBG_TRACE`）/ subclass 2（`DBG_TRACE_INFO`）里记录器自己发出的三类事件。
selected XML、线程链、POI 和帧覆盖再完整，也证明不了全局事件留存。这里补上采集与判读工具，
并写清楚它能证明什么、不能证明什么。

只用于以后另行冻结的诊断。它补不回旧 trace 里没有采到的事件，也不能用来宣布 TG 因果或
产品性能修复。trace、本机环境与测量数据只留本地（`target/sched_trace/`），这里只写方法、
结论和缺口。

```text
perf/sched_trace/
├── provider/v1/nana-trace-info.instrpkg   # Instruments 包：三张 ktrace 点表
├── provider/v1/manifest.json              # 源码 SHA、选择范围、缺旧基线说明
├── decode.py                              # build / record / decode / gate
└── loss_control.py                        # 过载对照负载
```

CPU 测试在 `scripts/tests/test_sched_trace.py`，CI 的 `unittest discover -s scripts/tests` 会跑到。

## Provider v1

| kind | kdebug code | SDK 名称 |
| --- | --- | --- |
| `lost` | 2 | `TRACE_LOST_EVENTS` |
| `retrograde` | 5 | `TRACE_RETROGRADE_EVENTS` |
| `past` | 6 | `TRACE_PAST_EVENTS` |

每个 `ktrace-point-schema` 的 pattern 固定 class `0x7`、subclass `0x2` 和自己的 code。function
qualifier、thread、CPU 和 arg1–arg4 都只绑定变量、不加约束。列按原值保存，不做 enum 映射，
也没有手写 modeler 或派生过滤（`instrumentbuilder` 会给 ktrace 点表自动生成一个 modeler，包源码注释
里的 “No modeler” 指的是手写的；v1 已冻结，注释不再改）。

原 provider 已从本地删除，也从未提交，所以 v1 是新定义，没有可逐行比对的旧基线。v1 发布后
冻结：要改就新开 `v2/`。`manifest.json` 固定源码 SHA-256，测试会核对。`instrumentbuilder`
两次构建同一源码产物不同，所以只固定源码。`record` 在源码 SHA 和 manifest 不一致时拒绝录制，
门禁也要求采集和对照的源码 SHA 都等于 manifest。

## 判读规则

解码器遇到任何一项不符就拒绝整份采集，不跳过单行：

- 列的 mnemonic、顺序和 engineering type 必须和 v1 完全一致；
- 每行恰好 9 格，code 必须和所在表一致，qualifier ∈ 0..3；
- 数值只接受规范的无符号十进制（不带空白、符号、前导零），`uint32`/`core` ≤ 2³²−1，`uint64` ≤ 2⁶⁴−1；
- 时间戳不晚于 toc 记录的时长，CPU 编号小于采集时记录的逻辑 CPU 数；
- 标量单元格不能有子元素，属性只能是 `id`/`fmt`；ref 单元格只能有 `ref` 属性；
- `id=`/`ref=` 必须先定义后引用，同名 id 不能定义两次，ref 的元素类型必须和定义一致；
- xctrace 的空值 `<sentinel/>` 必须为空，且只允许出现在 thread 列（标记由内核合成，不属于任何线程）
  和 CPU 列，解码为 `null`。CPU 为空在三张表里都允许；实测只在过载采集的 lost 表里出现过；
- 文档结构固定：一个 `<node>`，里面一个 `<schema>`（只含 `<col>`），后面只有 `<row>`；
- toc 只能有一个 run，toc 列出的表和实际导出的表必须一一对应。

每类事件的结果只有两种：

- `observed`：至少一行标记，附时间、CPU（可能未知）、线程、qualifier 和四个原始参数；
- `unknown`：表不在（`table-absent`），或表在但没有行（`no-marker-rows`）。

**没有“零丢失”这个结果。** 参数原样透传，没有任何字段把它解释成丢失条数。

工具能确认的：记录器在某个时刻、某个 CPU（可能未知）上报告了丢失、时间倒退或过去事件，以及报告时用的
分支参数。工具不能确认的：丢了多少条、丢了哪些事件、丢失区间从哪里开始。工具只采标记本身，
不采相邻事件，所以给不出丢失区间的边界。不发出 kdebug 标记的丢失路径（例如记录器在用户态
丢弃）完全看不到。

## 效果门禁

`decode.py gate CAPTURE --control CONTROL` 只有同时满足下面这些条件才返回 `open`（exit 0），
其余返回 `blocked`（exit 1）并列出原因；采集本身不合 v1 契约时直接 `rejected`（exit 2）：

1. 采集和对照的源码 SHA 都等于 manifest；
2. 录制 exit 0、已导出，toc 的结束原因是 `Time limit reached`；带负载时，负载是收到 tracing-started
   通知后才启动的，并且 exit 0；
3. 录制模式会保留整段（`Deferred` / `Immediate`），Windowed 不行；
4. 三张表都在，而且都没有标记行；
5. 对照采集本身满足 1–3，并且确实出现了 lost 标记，其中至少一条带 CPU（不带 CPU 的标记来源未证实）；
6. 对照和本次的配置完全一致：Instruments 版本、模板、录制模式、OS 版本、机型和逻辑 CPU 数。

门禁每次都从对照目录重新解码推导“收据”，不接受单独写出来的收据文件。它还会从两个目录里的
`capture.trace` 重新导出，和目录里的 `toc.xml`、各表 XML 比对（表逐字节比；toc 的进程列表顺序每次
导出都不同，所以按解码用到的字段比），对不上就 blocked。这只能把目录和它的 `.trace` 绑在一起，
`.trace` 本身仍然是可信的本地证据，工具无法鉴别伪造的 `.trace`。每份采集重新导出约需 1–2 分钟。
没有对照、表缺失、
录制截断或失败、换了任何一项配置，门禁都是 `blocked`，原效果门禁保持关闭。

门禁打开时会列出 `proven_visible_lost_branches`，也就是对照里实际见到的 `(qualifier, arg1..4)`。
只有这些分支被证明可见；其它分支和不发标记的丢失路径仍是 unknown。门禁打开只表示“这个配置下
没看到标记”，丢失条数依然无法证明。

## 采集

```bash
python3 perf/sched_trace/decode.py record --out target/sched_trace/v1/runs/<name> --time-limit 10s
python3 perf/sched_trace/decode.py decode target/sched_trace/v1/runs/<name>
python3 perf/sched_trace/decode.py gate target/sched_trace/v1/runs/<name> --control target/sched_trace/v1/runs/<control>
```

`record` 用 System Trace 模板、`--all-processes`，并通过 `--package` 和 `--instrument` 加载 v1。
`--` 后面可以跟一个负载命令；它要等 xctrace 发出 `--notify-tracing-started` 通知后才启动，
保证负载落在录制区间内，xctrace 失败时负载会被终止。输出目录已存在时直接报错，不覆盖旧采集。
`capture.json` 在导出之前就写下（`exported: false`），导出成功后再标成 `true`；录制或导出失败时，
退出码证据都还在。

磁盘前提：一次 10 秒的 Deferred 录制，`.trace` 约 1 GB；xctrace 另外会在 `$TMPDIR` 留一份
原始 `instruments*.ktrace`，约 8 GB，录完不删。可用空间不足时，xctrace 在保存阶段报
`Cannot save trace document`，随后 Instruments Packages 的 `NSDistributedLock` 抛异常。
这种失败要单独记录，不能当成有效采集。

## 记录器可见性（2026-10-23，Instruments 27.0）

System Trace 模板、`--all-processes`、v1 包、10 秒 time limit，三种条件用最终版工具各录一次，
都是 exit 0、`Time limit reached`、导出完整。这组之前的试录和一次因磁盘写满失败的录制都留在本地，
没有替换或删除：

| 条件 | 录制模式 | lost | retrograde / past | 门禁 |
| --- | --- | --- | --- | --- |
| 空闲 | Deferred | observed，分布在多个 CPU 上 | unknown（表在，无行） | blocked：观察到 lost |
| 过载（`loss_control.py`，每个逻辑 CPU 一个进程，tracing 开始后启动） | Deferred | observed，分布在多个 CPU 上，其中一条 CPU 为空 | unknown（表在，无行） | blocked：观察到 lost |
| `--window 1s` | Windowed (1 second) | unknown（表在，无行） | unknown（表在，无行） | blocked：模式不保留整段 |

结论：

- **Deferred 模式下 lost 标记看得见。** 空闲录制可以作为 `gate --control` 的正向对照目录。
- **这个配置下空闲也会报告 lost。** 所以现行的 System Trace / `--all-processes` / Deferred
  采集证明不了全局留存，原效果门禁保持关闭。
- 所有 lost 标记的参数都是 `(1, 0, 0, 0)`，qualifier 为 0，线程为空；这是唯一被证明可见的分支。
  arg1 是分支标记，不是条数；从标记数也推不出丢了多少事件。空闲和过载的标记数两次录制的
  大小关系相反，不能拿来比较负载大小。
- 篡改对照：复制空闲采集目录、删掉 `lost.xml` 里的全部行之后，解码结果变成 unknown，其余条件
  也都满足；门禁只靠从 `.trace` 重新导出核对才拦下它。三份真实采集的重新导出都和目录里的文件一致。
- 过载录制里有一条 lost 标记的 CPU 为空，并且和另一条带 CPU 的标记时间戳相同。解码器把它
  当作 CPU 未知的标记保留下来，不推断它属于哪个 CPU。
- **Windowed 模式是反例。** 它按设计丢弃窗口之前的全部事件，lost 表却是 0 行，所以“没看到
  标记”不等于“没有丢失”。Windowed 采集不能当对照，也不能打开门禁。
- retrograde 和 past 三种条件下都没有出现。没有能触发它们的对照，所以它们的可见性仍是
  unknown；门禁只要求这两张表存在且无行，不把无行当成“可见且为零”。

## 缺口

- 没有 root，做不了 `ktrace trace` 原始对照，无法从内核侧独立核对 xctrace 导出的标记是否完整。
- 没有找到能让空闲 lost 标记消失的采集配置（比如更窄的模板或进程范围）。在找到之前，
  门禁不会为任何真实采集打开。
- retrograde / past 缺正向对照：三种条件下 code 5 / 6 都是 0 行，所以这两种事件的原生选择是否
  生效还没有实证，只有 CPU 侧的选择器测试。
- 只在一台 Apple Silicon Mac、一个 Instruments 版本上测过；换版本、换模式或换机器都需要新的对照采集。
