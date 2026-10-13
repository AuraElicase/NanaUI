# 响应式与视图诊断码

下表列出编写声明式视图与响应式状态时可能遇到的运行期故障码（fault）。关于诊断底座的缓冲机制、`.nlog` 格式以及为什么严禁在帧路径上执行文件 I/O，请参见 [诊断与应用路径](diagnostics.md)。

| 故障码（Descriptor Name） | 触发条件 | 修复方法 |
| --- | --- | --- |
| `runtime.reactive.disposed_access` | 在作用域（如列表行或条件分支）已被销毁回收后，仍然读写了归属于该作用域的信号（同时触发 panic，报错信息中包含该信号的创建源码位置）。 | 在 `each` 行或 `when` 分支内创建的信号不要泄露到外部长期持有；异步回调或延迟清理任务请改用 `try_update` / `try_set_if_changed`（信号存活时才写入，已销毁则安全返回 `false`）。详见 [挂载与卸载](../guide/essentials/lifecycle.md)。 |
| `runtime.reactive.did_not_settle` | 响应式副作用（`watch_effect`）互相循环触发，连续超过 64 轮仍未收敛（剩余待执行队列会被丢弃以防死锁）。 | 避免在同一个 `watch_effect` 中无条件写入它自身读取的那个信号；如需派生新值请改用 `computed`，或使用 `set_if_changed` 截断相同值写入。详见 [侦听](../guide/essentials/watch.md)。 |
| `runtime.reactive.static_deps_mismatch` | 在 debug 构建下，`.vue` 编译器静态分析声明了依赖列表的绑定，在实际执行时读取了静态声明之外的信号（报错含模板行列位置；release 构建不做此检查）。 | 确保模板表达式所读取的信号都显式出现在编译器可见的作用域内，不要在被模板调用的普通黑盒函数里隐式读信号。详见 [.vue 方言](../guide/scaling/sfc.md)。 |
| `runtime.view.error_unhandled` | 视图构建闭包返回了 `Err`，但外层没有挂载 `error_boundary` 捕获它。 | 在上层包裹 `error_boundary`（或 `<ErrorBoundary>`）提供回退界面，或者在视图内部自行处理错误。详见 [声明式视图](reactive-view.md)。 |

除上述故障码外，`runtime.reactive` 域还提供一组零 I/O 的原子计数与直方图指标，用于观测响应式系统的负载：`runtime.reactive.flushes`、`runtime.reactive.signal_writes`、`runtime.reactive.effects_run`、`runtime.reactive.nodes_patched`、`runtime.reactive.commits` 以及耗时直方图 `runtime.reactive.flush`。
