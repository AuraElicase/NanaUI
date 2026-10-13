---
title: 代码阅读指南
---

# 代码阅读指南

这一章回答一个很实际的问题:**这个仓库该从哪里开始读,按什么顺序读,读到哪一步算「懂了」**。它是一张地图,不是教程——教程在前面各章。

## 先读文档,再读代码

按这个顺序建立心智模型,每篇都很短:

| 顺序 | 文件 | 给你什么 |
| --- | --- | --- |
| 1 | `AGENTS.md`(仓库根) | 全部架构合同的浓缩:产品路径、支撑边界、不可协商条款 |
| 2 | `docs/reference/how-it-works.md` | 心智模型:一棵树、宿主拥有 GPU、一帧的 8 步 |
| 3 | `docs/reference/architecture.md` | crate 边界与所有权表 |
| 4 | 本板块 [模块与分层](/learn/modules-and-layers) | 28 个 crate 的依赖层级实读 |
| 5 | [合同与不变量](/learn/contracts-and-invariants) | 9 条不变量及其核查方法 |

`AGENTS.md` 值得反复回看:仓库里几乎所有「为什么这段代码长这样」的答案,最后都指回其中一条合同。

## 仓库地形

```text
NanaUI/
├── Cargo.toml            workspace:28 个 crate + tools + platform + 8 个示例
├── AGENTS.md             架构合同(最重要的一份文件)
├── crates/               全部框架 crate(见下表)
├── examples/             8 个 workspace 成员示例(component-gallery、vue-v8、
│                         vue-counter、vue-hosted-acceptance、native-rhi-probe、
│                         runtime-host-fixture、package-fixture、reactive-sfc)
├── crates/nana-ui/examples/   16 个最小示例文件(counter/gpu/window 各种 probe)
├── docs/                 本文档站(VitePress)
├── packages/             JS 侧运行时包(nanavue-runtime 等,Vue 路径用)
├── platform/android      Android 宿主
├── tools/css-parity      CSS 子集一致性测试
├── perf/                 性能基线与门禁
└── scripts/check-engine-boundary.py   wgpu 边界静态检查
```

## 推荐阅读路线

### 路线一:一帧的生命周期(渲染主线)

1. `docs/reference/how-it-works.md` 的「一帧」一节——8 步总览。
2. [Scene 提取](/learn/scene-extraction):树怎么变成绘制图。
3. [GPU 与帧](/learn/gpu-frame):`GpuContext`/`FrameContext` 合同与 `SceneWgpuPainter`。
4. 对应源码:`crates/nana-ui-scene/src/`(绘制图)→ `crates/nana-gpu/src/`(合同)→ `crates/nana-ui/src/`(painter 与宿主适配)。

### 路线二:一棵树怎么被改动(状态主线)

1. [保留树](/learn/runtime-tree):`UiWorld`、`UiMutation`、flush 顺序。
2. [布局引擎](/learn/runtime-layout):样式怎么变成几何。
3. 对应源码:`crates/nana-ui-runtime/src/`(从 `lib.rs` 的模块声明进,`mutation.rs:15` 的 `UiMutation` 枚举是树操作的完整词表)。

### 路线三:一个点击怎么变成回调(输入主线)

1. [输入链路](/learn/runtime-input)。
2. 锚点源码:`crates/nana-ui-input/src/canonical.rs`(规范事件)→ `crates/nana-ui-runtime/src/framework/input.rs:397`(`AppContext::route_input`,唯一路由入口)。

## 最小可运行示例

```bash
# Rust 入口示例(how-it-works.md 引用的完整示例)
cargo run -p nana-ui --example application-counter

# 其他小示例都在这里
ls crates/nana-ui/examples/

# Vue 路径
cargo run -p vue-counter        # examples/vue-counter
```

16 个 `crates/nana-ui/examples/*.rs` 每个都只演示一件事:`gpu-view-demo`(GPU 内容挂树)、`transparent-window`(透明窗口)、`startup-splash`(两阶段启动)、`accessibility-hidden-probe`(无障碍)……名字即目录。

## 调试与诊断

- **框架诊断**走 `nana-diagnostics` 的 `event!` / `metric!` / `fault!`,静态描述符、ID 只增不减;诊断码表见 `/reference/errors`,机制见 `/reference/diagnostics`。帧路径上没有日志 crate、没有 `eprintln!`、没有 IO——找问题时先想到 metric 而不是日志。
- **热重载**:开发面见 `/guide/scaling/hot-reload` 与 `/reference/hot-reload`(nana-ui-dev)。
- **边界检查**:`python scripts/check-engine-boundary.py` 强制 `wgpu::*` 只在允许的 crate 公共签名出现、且必须开 `wgpu-interop`——改 GPU 相关代码后跑它。
- **CSS 一致性**:`tools/css-parity` 对照浏览器行为。

## 验证你的理解

读到这里,用三个问题自测(答案都在本板块):

1. 一个 `Button` 的点击,从操作系统事件到 `on_activate` 回调,经过哪些 crate?(答:[输入链路](/learn/runtime-input))
2. 一块实时 GPU 画面为什么会被其他控件遮挡、还能被点到?(答:[GPU 与帧](/learn/gpu-frame))
3. 为什么 runtime 的依赖表里没有 nana-ui-css,Vue 却能写 CSS?(答:[CSS 引擎](/learn/css-engine))

## 修改代码前

`AGENTS.md` 的「Change and validation hygiene」一节列了完整清单。最低限度:

```bash
cargo fmt --all -- --check
git diff --check          # 文档/技能改动
```

公开或跨层合同变化时,检查消费者、文档、features、manifest、lockfile 与测试——这不是风格建议,是合同。

## 接着读

- [端到端数据流](/learn/flows):四条完整链路串起全部章节
- [覆盖矩阵](/learn/coverage-matrix):28 个 crate 四要素速查
