# 模块与分层

本章回答三个基础问题：**工作区内的 28 个 crate 各自处于哪一层**、**依赖边如何单向流动**，以及**如何用一条命令亲手验证分层结论**。读完后你应能在脑中画出整个仓库的依赖地图，并能立即判断「某条新依赖该不该加」。

分层在 NanaUI 中不是形式主义，而是用编译期可见性守住三条底线：**只有一棵权威保留树**、**每个支撑引擎只有一处实现**、**核心树与场景对图形后端中立**。一旦依赖方向倒置，第二套权威就会随之滋生。

## 核心结论

工作区内 28 个 crate 的普通依赖（normal dependencies）全部沿「上层 → 下层」单向流动，**严格无环**。产品渲染主线分为四层：

```text
nana-ui-core → nana-ui-runtime → nana-ui-scene → nana-ui
  共享合同         保留树权威         保留绘制图      宿主适配 + SceneWgpuPainter
```

- `nana-ui-core` 的依赖表中没有 `runtime`、`scene` 或 `nana-ui`；
- `nana-ui-scene` 的依赖表中没有 `nana-ui`，也没有任何 GPU crate；
- 整张依赖图拓扑排序成功，不存在任何环路。

仅用于单元测试与基准测试的 `dev-dependencies` 有少量「向上」指向宿主层的边，已单独列在文末的 [dev 边与例外](#dev-边与例外)，不参与产品二进制分层。

## 如何用命令验证

全部分层读数来自在工作区根目录执行的一条标准命令：

```bash
cargo metadata --format-version 1
```

输出中的 `resolve.nodes` 列出了每个包实际解析出的依赖及 `dep_kinds`（可精确区分 `normal`、`dev` 与 `build`）。过滤出每个工作区成员指向 `crates/` 内路径包的 `normal` 依赖边，即可得到全仓 **77 条普通依赖边**（28 个成员 crate 之间 76 条 + `nana-ui → nana-icons-tabler` 1 条）。

## 依赖层级总表（Level 0 → Level 9）

下表按依赖深度归组排布，满足一条严格的不变量：**每一条非 optional 的普通依赖边都指向层级号更小的 crate**。仅有的两条同层依赖均为 feature 门控的 optional 依赖：`nana-app-icon → nana-ui-platform`（第 3 层，`runtime` feature，`crates/nana-app-icon/Cargo.toml:14,22`）与 `nana-ui-dev → nana-ui-devtools`（第 9 层，`headless` feature，`crates/nana-ui-dev/Cargo.toml:24,43`）。

| 层级 | Crate | 工作区内直接依赖（normal） | 核心角色 |
| --- | --- | --- | --- |
| 0 | `nana-diagnostics` | — | 框架诊断底座：`event!` / `metric!` / `fault!` / `span!` |
| 0 | `nana-svg-raster` | — | CPU 侧 SVG 栅格化工具 |
| 0 | `nana-package` | — | 资源包（`.nrpack`）与清单的运行时只读模型 |
| 0 | `nana-ui-view-schema` | — | 声明式视图内置控件的唯一 schema 表 |
| 0 | `nana-js-engine` | — | JS 引擎中立合同与 `HostApiRegistry` |
| 1 | `nana-ui-core` | `nana-diagnostics` | 共享基础合同：Style Model、主题、几何、WorkspaceModel、Motion IR |
| 2 | `nana-text` | `nana-ui-core` | 文本测量、OpenType 整形与保留文本布局（产品唯一排版权威） |
| 2 | `nana-ui-css` | `nana-ui-core` | CSS 解析、选择器匹配与级联的唯一实现 |
| 2 | `nana-ui-charts` | `nana-ui-core` | 图表模型与 GPU 标记数组 |
| 2 | `nana-ui-input` | `nana-ui-core`, `nana-diagnostics` | 规范输入事件、序列器、有界端点与 `HostServices` |
| 2 | `nana-window` | `nana-ui-core` | 原生窗口句柄、系统材质、客户区 chrome、缩放 |
| 2 | `nana-gpu` | `nana-diagnostics` | GPU 后端合同：`GpuContext` / `FrameContext` / `GpuTexture` |
| 3 | `nana-ui-platform` | `nana-ui-core`, `nana-ui-input`, `nana-diagnostics` | 平台中立窗口合同（并再导出输入合同） |
| 3 | `nana-app-icon` | `nana-ui-platform`（optional，`runtime` feature） | 应用默认图标与三平台图标编码 |
| 3 | `nana-frame-exchange` | `nana-gpu` | 跨线程有界最新帧交换池 |
| 3 | `nana-ui-view-codegen` | `nana-ui-core`, `nana-ui-css`, `nana-ui-view-schema` | `view!` 与 `.vue` 的代码生成器（构建时调用 CSS 引擎） |
| 3 | `nana-js-v8` | `nana-js-engine` | 基于 V8 的 `JsEngine` 实现 |
| 4 | `nana-ui-web-api` | `nana-js-engine`, `nana-svg-raster`, `nana-text`, `nana-ui-core`, `nana-ui-platform` | JS 侧宿主 Web API 子集 |
| 4 | `nana-ui-view-macros` | `nana-ui-view-codegen` | `view!` / `css!` / `stylesheet!` 过程宏 |
| 4 | `nana-ui-sfc` | `nana-ui-view-codegen` | `.vue` 单文件组件（Rust 脚本）构建期编译器 |
| 5 | `nana-ui-runtime` | `nana-diagnostics`, `nana-ui-core`, `nana-text`, `nana-ui-charts`, `nana-ui-input`, `nana-ui-view-macros`, `nana-ui-view-schema` | **`UiWorld` 保留树权威**、内置控件、布局引擎、输入路由、Shell/Dock/AreaLayout |
| 6 | `nana-ui-scene` | `nana-diagnostics`, `nana-ui-core`, `nana-ui-charts`, `nana-ui-runtime`, `nana-text`（可选 benchmark） | `UiScene` 保留绘制图与 `FramePlan`（不依赖 WGPU） |
| 7 | `nana-ui` | `nana-app-icon`, `nana-diagnostics`, `nana-frame-exchange`, `nana-gpu`, `nana-icons-tabler`, `nana-package`, `nana-svg-raster`, `nana-text`, `nana-ui-charts`, `nana-ui-core`, `nana-ui-platform`, `nana-ui-runtime`, `nana-ui-scene`, `nana-window` | 宿主适配器、`SceneWgpuPainter`、`run_runtime` |
| 8 | `nana-ui-vue` | `nana-app-icon`, `nana-gpu`, `nana-js-engine`, `nana-svg-raster`, `nana-ui`, `nana-ui-core`, `nana-ui-css`, `nana-ui-platform`, `nana-ui-runtime`, `nana-ui-scene`, `nana-ui-web-api` | Vue Custom Renderer → 写入同一棵 `UiWorld` |
| 9 | `nana-packager` | `nana-app-icon`, `nana-package` | 应用打包与防篡改校验 CLI |
| 9 | `nana-ui-dev` | `nana-js-engine`, `nana-ui`, `nana-ui-core`, 可选 `nana-ui-devtools` / `nana-ui-sfc` / `nana-ui-vue` | 开发期热重载工具（仅限 debug 构建） |
| 9 | `nana-ui-devtools` | `nana-gpu`, `nana-js-engine`, `nana-js-v8`, `nana-ui`, `nana-ui-core`, `nana-ui-platform`, `nana-ui-scene`, `nana-ui-vue` | 开发期结构化诊断记录与无头 Agent 会话 |
| 独立 | `nana-icons-tabler` | `nana-ui-core`（path 依赖，非 workspace member） | 自动生成的 Tabler 图标常量目录 |

::: tip 层级号代表依赖深度，而非“重要程度”
`nana-ui-core` 位于第 1 层，却是全框架最核心的数据合同；层级号越大，代表它越靠近宿主组装层与应用入口。
:::

## 按职能分为六大板块

1. **产品渲染主线（一帧的骨架）**：`输入（Rust / Vue / JS） → nana-ui-core → nana-ui-runtime → nana-ui-scene → nana-ui → 宿主 Surface`。
2. **六大支撑引擎（各自唯一权威）**：
   - 文本引擎：`nana-text`（被 `runtime` 测量、`nana-ui` 绘制、`web-api` Canvas 消费）；
   - CSS 引擎：`nana-ui-css`（仅被构建期的 `view-codegen` 与运行期的 `nana-ui-vue` 消费，`runtime` 与 `nana-ui` 均不依赖它）；
   - 图表模型：`nana-ui-charts`（被 `runtime`、`scene` 与 `nana-ui` 管线消费）；
   - 输入合同：`nana-ui-input`（经 `platform` 再导出，由 `runtime` 唯一消费路由）；
   - GPU 合同：`nana-gpu`（被 `nana-ui`、`vue`、`frame-exchange`、`devtools` 消费）；
   - 原生窗口：`nana-window`（被 `nana-ui` 宿主层调用）。
3. **JS / Vue 宿主适配面**：`nana-js-engine`（合同）→ `nana-js-v8`（V8 实现）→ `nana-ui-web-api`（宿主 Web API）→ `nana-ui-vue`（Vue Custom Renderer）。
4. **声明式视图编译工具链**：`nana-ui-view-schema` → `nana-ui-view-codegen` → `nana-ui-view-macros` 与 `nana-ui-sfc`（详见 [作者工具链](/learn/toolchain)）。
5. **资产、图标与打包分发**：`nana-svg-raster`、`nana-app-icon`、`nana-icons-tabler`、`nana-package`（运行时读侧）、`nana-packager`（构建期写侧）。
6. **开发期与自动化验证工具**：`nana-ui-dev`（热重载）、`nana-ui-devtools`（无头 Agent 会话与像素/语义快照）。

## 77 条普通依赖边完整清单

```text
nana-diagnostics     → （零工作区依赖）
nana-svg-raster      → （零工作区依赖）
nana-package         → （零工作区依赖）
nana-ui-view-schema  → （零工作区依赖）
nana-js-engine       → （零工作区依赖）
nana-icons-tabler    → nana-ui-core（非 member 的 path 依赖，不计入 28 成员起点；由链接器按引用裁剪）
nana-ui-core         → nana-diagnostics
nana-text            → nana-ui-core
nana-ui-css          → nana-ui-core
nana-ui-charts       → nana-ui-core
nana-ui-input        → nana-ui-core, nana-diagnostics
nana-window          → nana-ui-core
nana-gpu             → nana-diagnostics
nana-ui-platform     → nana-ui-core, nana-ui-input, nana-diagnostics
nana-app-icon        → nana-ui-platform（optional，feature `runtime`）
nana-frame-exchange  → nana-gpu
nana-ui-view-codegen → nana-ui-core, nana-ui-css, nana-ui-view-schema
nana-js-v8           → nana-js-engine
nana-ui-web-api      → nana-js-engine, nana-svg-raster, nana-text, nana-ui-core, nana-ui-platform
nana-ui-view-macros  → nana-ui-view-codegen
nana-ui-runtime      → nana-diagnostics, nana-ui-core, nana-text, nana-ui-charts,
                       nana-ui-input, nana-ui-view-macros, nana-ui-view-schema
nana-ui-sfc          → nana-ui-view-codegen
nana-ui-scene        → nana-diagnostics, nana-ui-core, nana-ui-charts, nana-ui-runtime,
                       nana-text（benchmark feature，可选）
nana-ui              → nana-app-icon, nana-diagnostics, nana-frame-exchange, nana-gpu,
                       nana-icons-tabler, nana-package, nana-svg-raster, nana-text,
                       nana-ui-charts, nana-ui-core, nana-ui-platform, nana-ui-runtime,
                       nana-ui-scene, nana-window
nana-ui-vue          → nana-app-icon, nana-gpu, nana-js-engine, nana-svg-raster,
                       nana-ui, nana-ui-core, nana-ui-css, nana-ui-platform,
                       nana-ui-runtime, nana-ui-scene, nana-ui-web-api
nana-packager        → nana-app-icon, nana-package
nana-ui-dev          → nana-js-engine, nana-ui, nana-ui-core, nana-ui-devtools（optional，feature `headless`）,
                       nana-ui-sfc（optional，feature `templates`）, nana-ui-vue（optional，feature `vue`）
nana-ui-devtools     → nana-gpu, nana-js-engine, nana-js-v8, nana-ui, nana-ui-core,
                       nana-ui-platform, nana-ui-scene, nana-ui-vue
```

## dev 边与例外

以下 `dev-dependencies` 仅服务于各 crate 自身的集成测试与基准测试，不会进入任何产品构建产物：

```text
nana-js-engine      → nana-js-engine（dev，自测）
nana-js-v8          → nana-js-engine, nana-ui, nana-ui-platform, nana-ui-vue, nana-ui-web-api（dev，端到端宿主测试）
nana-text           → nana-ui-core（dev，bundled-fonts 测试语料）
nana-ui-scene       → nana-text, nana-ui-core, nana-ui-input（dev，构造基准与场景测试）
nana-packager       → nana-ui-platform（dev）
nana-ui-dev         → nana-ui-platform, nana-ui-runtime, nana-ui-scene（dev）
```

## 给学习者的三条判断法则

1. **想找某件事的权威实现，先看依赖表**：`nana-ui-runtime` 依赖 `nana-text` 而非相反，说明文本排版权威在 `nana-text`；`nana-ui` 位于第 7 层依赖各子系统，说明它是宿主组装层而非树或样式权威。
2. **反向依赖等于滋生第二套权威**：如果哪天 `nana-ui-core` 依赖了 `nana-ui-runtime`，基础合同层就会耦合保留树的内部结构，分层屏障随之失效。
3. **一个 crate 所处的层级决定了它的视野**：第 2 层的 `nana-ui-css` 看不见树和窗口，因此不可能在 CSS 引擎里偷偷测量容器；第 6 层的 `nana-ui-scene` 看不见 `nana-gpu`，因此绘制图天然与 WGPU 解耦。

## 接着读

- [合同与不变量](/learn/contracts-and-invariants)：分层所守护的九条核心架构合同
- [28 crate 覆盖矩阵](/learn/coverage-matrix)：每个 crate 的关键入口类型与源码锚点速查
- [架构合同（参考）](/reference/architecture)：面向维护者的边界合同全文
