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
├── control/v1/nana-trace-load-sink.instrpkg  # 仅用于对照：记录负载自己的 kdebug 事件
├── control/v1/manifest.json               # 源码 SHA 与选择范围
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
3. 录制模式会保留整段（`Deferred` / `Immediate`），Windowed 不行。实测 xctrace 命令行录出来的都是
   Deferred，见下文“收窄 Thread Activity 的尝试”；
4. 三张表都在，而且都没有标记行；
5. 对照采集本身满足 1–3，并且确实出现了 lost 标记，其中至少一条带 CPU（不带 CPU 的标记来源未证实）；
6. 对照和本次的配置完全一致：Instruments 版本、模板、额外加载的 instrument 集合、toc 列出的
   schema 集合、time limit、录制模式、OS 版本、机型和逻辑 CPU 数。instrument 集合是 `capture.json`
   自述的，toc 的 schema 集合则绑定在 `.trace` 上，两者一起比，改自述也配不上另一种配置。加载了
   load sink 的采集，sink 源码 SHA 也必须等于它的 manifest。

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

`record` 默认用 System Trace 模板、`--all-processes`，并通过 `--package` 和 `--instrument` 加载 v1。
`--template Blank` 表示不用模板、只加载 instrument；`--instrument NAME` 可重复，追加 Instruments
自带的 instrument；`--load-sink` 追加对照用的 load sink（见下）。
`--` 后面可以跟一个负载命令；它要等 xctrace 发出 `--notify-tracing-started` 通知后才启动，
保证负载落在录制区间内，xctrace 失败时负载会被终止。输出目录已存在时直接报错，不覆盖旧采集。
`capture.json` 在导出之前就写下（`exported: false`），导出成功后再标成 `true`；录制或导出失败时，
退出码证据都还在。

磁盘前提：一次 10 秒的 Deferred 录制，`.trace` 约 1 GB；xctrace 另外会在 `$TMPDIR` 留一份
原始 `instruments*.ktrace`，约 8 GB，录完不删。可用空间不足时，xctrace 在保存阶段报
`Cannot save trace document`，随后 Instruments Packages 的 `NSDistributedLock` 抛异常。
这种失败要单独记录，不能当成有效采集。

## 记录器可见性：System Trace（2026-10-23，Instruments 27.0）

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

## Load sink 对照（control/v1）

System Trace 的空闲 lost 来自它自己的高频 instrument，所以要找更窄的配置。窄配置下
`loss_control.py --mode syscall` 产生的系统调用不会被记录，制造不出真实丢失；Points of Interest
也不记录 kdebug signpost。为此加了一个只用于对照的包 `control/v1/nana-trace-load-sink.instrpkg`：
一张 ktrace 点表，pattern 只匹配 `loss_control.py --mode kdebug` 自己发的事件（class `0x21` /
subclass `0xa` / code `0x359` / function 0）。内核按 class/subclass 开启记录，所以同一 subclass 下
别的 signpost 也会进缓冲区。它不选 class 7，也不改 v1。sink 表本身不导出；“丢失是负载灌满缓冲区
造成的”是从同配置空闲与负载的对比推出来的，没有逐条核对。负载的任一进程非零退出（含被信号终止）
都算负载失败。它同样冻结，`manifest.json` 固定源码 SHA，
`record --load-sink` 在 SHA 不符时拒绝录制。

用户态伪造：tracing 开启时，用户进程调用 `kdebug_trace()` 发 class 7 返回 `EPERM`，发 `DBG_APPS`
则成功。所以负载只能让缓冲区溢出，不能直接写出 lost 标记；这也意味着 retrograde / past 无法从
用户态构造正向对照。

## 记录器可见性：窄配置（2026-10-23，Instruments 27.0）

都是 `--all-processes`、v1 包、10 秒 time limit、Deferred，全部 exit 0、`Time limit reached`、导出完整。

先把 System Trace 拆开，每次 `--template Blank` 加一个 instrument（空闲，各录一次）：

| 配置 | 空闲 lost |
| --- | --- |
| Blank（只有 v1） | unknown（无行） |
| + Thread Activity | observed |
| + System Call Trace | observed |
| + Virtual Memory Trace / Time Profiler / CPU Profiler / Points of Interest / Hangs / Run Loops / os_signpost（各自单独） | unknown（无行） |

没有对照之前，这些“无行”都只是 unknown，不能当成没有丢失。于是在两种配置上加 load sink，
按“空闲 / 负载”交替录制：

| 配置 | 空闲 | `--mode kdebug` 负载（进程数） | 门禁（空闲对同配置负载） |
| --- | --- | --- | --- |
| Blank + sink | 4 次全部无行 | 1、2、4 和每个逻辑 CPU 一个，各次都 observed | open（3 组对照各一次） |
| Blank + Time Profiler + sink | 1 次无行 | 每个逻辑 CPU 一个，observed | open |

结论：

- **门禁第一次在真实采集上打开**，但只在这两种窄配置下：空闲采集三张表都在、都无行，同配置的
  负载对照确实出现了 lost。被证明可见的分支仍然只有 `(qualifier 0, 1, 0, 0, 0)`，负载对照里的标记
  都带 CPU，线程为空。
- 负载哪怕只有一个进程也会溢出，标记数随进程数增加，但标记数不是丢失条数，不能拿来量化。
- 反例都按预期拦下：配置不同的交叉对照（空闲 Blank + sink 对 Blank + Time Profiler + sink 的负载）
  因 instrument 集合不同而 blocked；把负载采集本身拿去过门禁，因观察到 lost 而 blocked。
- 这些配置几乎不记录调度信息。门禁 open 只说明“这个配置、这台机器、这段空闲时间里，已证明
  可见的那个 lost 分支没有出现”，不说明 System Trace 或任何调度研究配置的留存。真正的诊断负载要用
  它自己的配置和自己的对照重新过门禁；从 Thread Activity、System Call Trace 的结果看，
  带这两个 instrument 的配置空闲时就会丢失。

### 收窄 Thread Activity 的尝试

这一轮想找一个仍带 Thread Activity、空闲时又不丢失的配置。做法是直接调用 xctrace，每次录 2–5 秒，
都用 v1 包，再用同一个严格解码器和门禁函数判读。结果如下：

| 尝试 | toc 里的模式 / 结束原因 | 空闲 lost |
| --- | --- | --- |
| Blank + Thread Activity，`--all-processes` | Deferred / Time limit reached | observed |
| 同上，`--attach` 一个空闲进程 | Deferred / Time limit reached | observed，单位时间的标记数不比全进程少 |
| 同上，`--launch` 一个比时限短的进程 | Deferred / `Target app exited` | observed；目标一退出录制就结束，门禁不认这个结束原因 |
| 同上，`--launch` 一个比时限长的进程 | Deferred / Time limit reached | observed；xctrace 在时限到时杀掉目标，自己 exit 54 |
| Time Profiler 模板（模板里写的是 Immediate）+ Thread Activity | Deferred / Time limit reached | observed |
| 把 Blank 模板复制一份、`recordingMode` 改成 Immediate，再加 Thread Activity | Deferred / Time limit reached | observed |
| 同一份改过的模板，只加 v1 | Deferred / Time limit reached | unknown（无行） |

结论：

- **Thread Activity 不按目标进程收窄。** attach 和 launch 都只改变“录谁”，内核事件照样按全系统采集，
  空闲丢失不减少。
- **xctrace 命令行录不出 Immediate。** 不论模板里写什么，toc 都记 Deferred。xctrace 没有调缓冲区
  大小的选项，`--show-recording-options` 对 Thread Activity 也给不出可调项。
- 所以在这台机器、这个 Instruments 版本上，只要带 Thread Activity，空闲时就会丢失，门禁必然
  blocked，加负载对照也改变不了这一点，因此没有再录负载对照。这些探测不经过 `record`，没有
  `capture.json`，只作为可行性证据，不能当门禁的输入。

## 源码依据（xnu `bsd/kern/kdebug.c`）

公开的 xnu 源码当前是 `xnu-12377.1.9`；本机内核是 `xnu-13432.1.9`，这个版本的源码还没公开，
所以下面这些只是参考，不是对本机内核的证明（`bsd/kern/kdebug.c` 的读取函数，`kdebug_common.c` 的
`kdebug_storage_alloc`）。三种标记都由读取端合并各 CPU 缓冲区时合成，不是事件发生当时写下的：

- **lost，`arg1 = 1`**：空闲存储单元用完、允许回卷时，内核回收最旧的存储单元，覆盖还没读出的事件。
  读取端遇到被回收的单元就发这个标记，时间戳是新的读取下界，CPU 号是被回收的 CPU；如果下界没有
  前移，CPU 号字段是 -1。实测那条 CPU 为空的标记可能就是这种情况，未证实。实测见到的全是这个分支。
- **lost，`arg1 = 0`**：缓冲区已经整体回卷时，读取端只发一条，不给 CPU 号赋值（为 0，不代表真实
  CPU）。所有采集里一次都没见过，可见性 unknown。
- 两个 lost 分支都只在允许回卷时发出。不允许回卷时，存储单元用完后新事件直接丢弃，内核只置一个
  “已回卷”状态位，读取端不发任何标记；这又是一条看不见的丢失路径。启动早期缓冲区溢出另有一条
  `arg1 = 1` 的 lost，和录制无关。
- **past**：协处理器带显式时间戳写入的事件早于读取下界时，事件本身被丢弃，读取端补一条 past，
  `arg1` 是被丢弃事件里最晚的时间戳。所以 past 也表示丢失。
- **retrograde**：同一个 CPU 上读出的时间戳比前一条早时触发，每次读取调用最多报一条；`arg1` 取自
  本批第一条事件的 debugid，`arg2` 是倒退后的时间戳。
- 能主动构造 past 的测试钩子只编进 DEVELOPMENT/DEBUG 内核，而且要 root。本机是 RELEASE 内核，
  用户态又不能发 class 7，所以 retrograde / past 在这台机器上构造不出正向对照。

## 缺口

- 没有 root，做不了 `ktrace trace` 原始对照，无法从内核侧独立核对 xctrace 导出的标记是否完整。
- 空闲时不丢失、门禁能打开的配置目前只有不记录调度的窄配置（见上）。带 Thread Activity 或
  System Call Trace 的配置空闲时就会丢失；attach、launch 和 Immediate 都试过，收窄不了，所以
  调度研究需要的配置在这台机器上没有能打开门禁的。
- retrograde / past 缺正向对照：所有采集里 code 5 / 6 都是 0 行，触发条件又只在内核和协处理器路径上
  （见“源码依据”），所以这两种事件的原生选择是否生效还没有实证，只有 CPU 侧的选择器测试。要补，
  需要 DEVELOPMENT 内核加 root，或者改用源码比对作为验收依据。
- lost 只见过 `arg1 = 1` 这个分支 `(0, 1, 0, 0, 0)`；`arg1 = 0`（预期回卷）和不发标记的丢失路径
  （不回卷时停录、记录器在用户态丢弃）仍是 unknown。
- 只在一台 Apple Silicon Mac、一个 Instruments 版本上测过；换版本、换模式或换机器都需要新的对照采集。
