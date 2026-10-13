# 模块与分层

这一章回答:28 个 crate 各自在哪一层、依赖怎么流、怎么用命令亲自验证分层结论。读完你应能在脑中画出这个仓库的依赖地图,并能判断「一个新依赖该不该加」。

分层不是风格偏好。NanaUI 用分层守住三件事:**只有一棵权威树**、**每个引擎只有一处实现**、**绘制后端中立**。依赖一旦反向,权威就会长出第二套。

## 先看结论

工作区内 28 个 crate 的普通依赖(normal)全部沿「上层 → 下层」单向流动,**无环**。产品路径四层:

```text
nana-ui-core → nana-ui-runtime → nana-ui-scene → nana-ui
   合同          树权威             绘制图          宿主适配 + painter
```

- `nana-ui-core` 的依赖表里没有 runtime/scene/nana-ui;
- `nana-ui-scene` 的依赖表里没有 nana-ui,也没有任何 GPU crate;
- 整张图拓扑排序成功,没有环。

dev 依赖(仅测试/示例使用)有少量「向上」的边,单独列在[后面](#dev-边与例外),不参与产品层位。

## 怎么验证

全部读数来自一条命令(`cargo metadata`,在工作区根目录执行):

```bash
cargo metadata --format-version 1
```

它的 `resolve.nodes` 列出每个包实际解析出的依赖(含 `dep_kinds`,可区分 normal / dev / build)。解析脚本只做一件事:从每个 workspace member 的普通依赖里,留下**终点是 crates/ 内路径包**的边(成员之间 76 条,外加 `nana-ui → nana-icons-tabler` 一条——tabler 不是 member,是路径依赖包)。你也可以逐个读 `crates/*/Cargo.toml` 对照——metadata 的好处是**解析后的真实图**,不受 feature 组合与平台条件的干扰(以当前编译目标为准)。

## 层级总表

层级号按依赖深度归组排布,不变量是:**每条非 optional 的普通依赖边都指向更小的层级号**。两条同层边都是 feature 门控的 optional 依赖:`nana-app-icon → nana-ui-platform`(第 3 层,feature `runtime`,`crates/nana-app-icon/Cargo.toml:14,22`)与 `nana-ui-dev → nana-ui-devtools`(第 9 层,feature `headless`,`crates/nana-ui-dev/Cargo.toml:24,43`)。层级号是阅读顺序而非严格度量:按纯最长路径公式,`nana-js-v8`(只依赖第 0 层的 `nana-js-engine`)应是第 1 层,与 `nana-ui-platform` 同列第 3 层是功能归组——先读谁,而不是精确深度。

| 层级 | Crate | 工作区内直接依赖(normal) | 角色 |
| --- | --- | --- | --- |
| 0 | nana-diagnostics | — | 框架诊断:`event!`/`metric!`/`fault!` |
| 0 | nana-svg-raster | — | SVG 栅格化工具 |
| 0 | nana-package | — | 包模型(分发用) |
| 0 | nana-ui-view-schema | — | `view!` 树的 schema 定义 |
| 0 | nana-js-engine | — | JS 引擎中立合同 |
| 1 | nana-ui-core | nana-diagnostics | 共享合同:Style Model、主题、几何、Workspace、Motion IR |
| 2 | nana-text | nana-ui-core | 文本测量/整形/保留布局(产品唯一排版权威) |
| 2 | nana-ui-css | nana-ui-core | CSS 解析/级联的唯一实现 |
| 2 | nana-ui-charts | nana-ui-core | 图表模型与 GPU 标记数组 |
| 2 | nana-ui-input | nana-ui-core, nana-diagnostics | 规范输入事件、序列器、HostServices |
| 2 | nana-window | nana-ui-core | 原生窗口、材质、chrome、缩放 |
| 2 | nana-gpu | nana-diagnostics | GPU 后端合同:GpuContext/FrameContext/GpuTexture |
| 3 | nana-ui-platform | nana-ui-core, nana-ui-input | 平台中立窗口合同(再导出 input) |
| 3 | nana-app-icon | nana-ui-platform(optional,feature `runtime`) | 应用图标定义 |
| 3 | nana-frame-exchange | nana-gpu | 跨线程最新帧槽池 |
| 3 | nana-ui-view-codegen | nana-ui-core, nana-ui-css, nana-ui-view-schema | `view!` 的代码生成(构建时用 CSS 引擎) |
| 3 | nana-js-v8 | nana-js-engine | V8 实现 JS 合同 |
| 4 | nana-ui-web-api | nana-js-engine, nana-svg-raster, nana-text, nana-ui-core, nana-ui-platform | JS 侧宿主 API |
| 4 | nana-ui-view-macros | nana-ui-view-codegen | `view!`/`css!` 过程宏 |
| 4 | nana-ui-sfc | nana-ui-view-codegen | `.vue` 单文件组件构建时编译 |
| 5 | nana-ui-runtime | nana-diagnostics, nana-ui-core, nana-text, nana-ui-charts, nana-ui-input, nana-ui-view-macros, nana-ui-view-schema | **UiWorld 保留树权威**、控件、布局、Shell/Dock |
| 6 | nana-ui-scene | nana-diagnostics, nana-ui-core, nana-ui-charts, nana-ui-runtime | UiScene 绘制图(不依赖 WGPU) |
| 7 | nana-ui | nana-app-icon, nana-diagnostics, nana-frame-exchange, nana-gpu, nana-package, nana-svg-raster, nana-text, nana-ui-charts, nana-ui-core, nana-ui-platform, nana-ui-runtime, nana-ui-scene, nana-window | 宿主适配器、SceneWgpuPainter、run_runtime |
| 8 | nana-ui-vue | nana-app-icon, nana-gpu, nana-js-engine, nana-svg-raster, nana-ui, nana-ui-core, nana-ui-css, nana-ui-platform, nana-ui-runtime, nana-ui-scene, nana-ui-web-api | Vue Custom Renderer → 同一棵树 |
| 9 | nana-packager | nana-app-icon, nana-package | 应用打包 |
| 9 | nana-ui-dev | nana-js-engine, nana-ui, nana-ui-core | 开发期工具(热重载等) |
| 9 | nana-ui-devtools | nana-js-engine, nana-ui-core | 调试器协议 |
| 独立 | nana-icons-tabler | nana-ui-core(path;非 workspace member) | Tabler 图标常量目录,生成物 |

::: warning
层级号是**依赖深度**,不是「重要性」。core 在第 1 层,却是最核心的合同层;层级高只意味着「离宿主/应用更近」。
:::

## 按职能分组

层级回答「依赖怎么流」,分组回答「这 crate 是干嘛的」。同一个 28 个 crate,按 AGENTS.md 的叙述可以分成六组:

### 1. 产品路径(一帧的骨架)

```text
输入(Rust/Vue/JS) → core → runtime → scene → nana-ui → 宿主 Surface
```

`nana-ui`(7 层)站在顶端,把 runtime/scene/gpu/window 等全部组装起来交给应用。

### 2. 支撑引擎(各只有一个权威实现)

| 引擎 | Crate | 被谁消费 |
| --- | --- | --- |
| 文本 | nana-text | runtime(测量)、nana-ui(绘制)、web-api(JS 文本) |
| 样式 | nana-ui-css | **view-codegen(构建时)、vue(运行时)**——注意 runtime 自己不依赖 css |
| 图表 | nana-ui-charts | runtime(Chart 组件)、scene、nana-ui(管线) |
| 输入 | nana-ui-input | platform(再导出)、runtime(唯一路由消费方) |
| GPU | nana-gpu | nana-ui、vue、frame-exchange |
| 窗口 | nana-window | nana-ui |

CSS 的消费关系值得单独看一眼:Rust 路径的 `view!` 在**构建时**由 view-codegen 调用 css 引擎,编译成 Style Model 数据;Vue 路径在**运行时**由 nana-ui-vue 调用。所以 `nana-ui-runtime` 和 `nana-ui` 的依赖表里都找不到 `nana-ui-css`——它们只消费 Style Model,不解析样式表。详见 [CSS 引擎](/learn/css-engine)。

### 3. 语言面(JS/Vue 宿主)

`nana-js-engine`(合同)→ `nana-js-v8`(V8 实现)→ `nana-ui-web-api`(JS 侧 API)→ `nana-ui-vue`(Vue Custom Renderer)。它们是**输入适配器**:写入同一棵 UiWorld,不开第二套窗口。详见[作者入口](/learn/authoring-paths)。

### 4. 编译工具链

`view-schema`(树 schema)→ `view-codegen`(代码生成)→ `view-macros`(过程宏,被 runtime 依赖)与 `nana-ui-sfc`(`.vue` 文件的构建时编译,被应用构建脚本使用)。详见[编译工具链](/learn/toolchain)。

### 5. 资产与分发

`nana-svg-raster`、`nana-app-icon`、`nana-package`、`nana-packager`、`nana-icons-tabler`。

### 6. 开发面

`nana-ui-dev`(开发期工具)、`nana-ui-devtools`(调试器协议)。

## 完整依赖边清单

普通依赖(normal)边,共 77 条(以 28 个成员 crate 为起点:成员之间 76 条 + `nana-ui → nana-icons-tabler` 1 条;不含 examples、platform/android、tools、fixtures;读数命令见本页末尾):

```text
nana-diagnostics     → (零工作区依赖)
nana-svg-raster      → (零工作区依赖)
nana-package         → (零工作区依赖)
nana-ui-view-schema  → (零工作区依赖)
nana-js-engine       → (零工作区依赖)
nana-icons-tabler    → nana-ui-core(非 member 的 path 依赖,不计入上面 77 条;链接器裁剪)
nana-ui-core         → nana-diagnostics
nana-text            → nana-ui-core
nana-ui-css          → nana-ui-core
nana-ui-charts       → nana-ui-core
nana-ui-input        → nana-ui-core, nana-diagnostics
nana-window          → nana-ui-core
nana-gpu             → nana-diagnostics
nana-ui-platform     → nana-ui-core, nana-ui-input, nana-diagnostics
nana-app-icon        → nana-ui-platform(optional,feature `runtime`)
nana-frame-exchange  → nana-gpu
nana-ui-view-codegen → nana-ui-core, nana-ui-css, nana-ui-view-schema
nana-js-v8           → nana-js-engine
nana-ui-web-api      → nana-js-engine, nana-svg-raster, nana-text, nana-ui-core, nana-ui-platform
nana-ui-view-macros  → nana-ui-view-codegen
nana-ui-runtime      → nana-diagnostics, nana-ui-core, nana-text, nana-ui-charts,
                       nana-ui-input, nana-ui-view-macros, nana-ui-view-schema
nana-ui-sfc          → nana-ui-view-codegen
nana-ui-scene        → nana-diagnostics, nana-ui-core, nana-ui-charts, nana-ui-runtime,
                       nana-text(benchmark feature,可选)
nana-ui              → nana-app-icon, nana-diagnostics, nana-frame-exchange, nana-gpu,
                       nana-icons-tabler, nana-package, nana-svg-raster, nana-text,
                       nana-ui-charts, nana-ui-core, nana-ui-platform, nana-ui-runtime,
                       nana-ui-scene, nana-window
nana-ui-vue          → nana-app-icon, nana-gpu, nana-js-engine, nana-svg-raster,
                       nana-ui, nana-ui-core, nana-ui-css, nana-ui-platform,
                       nana-ui-runtime, nana-ui-scene, nana-ui-web-api
nana-packager        → nana-app-icon, nana-package
nana-ui-dev          → nana-js-engine, nana-ui, nana-ui-core, nana-ui-devtools(optional,feature `headless`),
                       nana-ui-sfc(optional,feature `templates`), nana-ui-vue(optional,feature `vue`)
nana-ui-devtools     → nana-gpu, nana-js-engine, nana-js-v8, nana-ui, nana-ui-core,
                       nana-ui-platform, nana-ui-scene, nana-ui-vue
```

零依赖 crate(只被别人依赖):`nana-diagnostics`、`nana-svg-raster`、`nana-package`、`nana-ui-view-schema`、`nana-js-engine`(`nana-icons-tabler` 非 member,自身 path 依赖 core,不在本清单口径内)。

读数命令(可复跑;把 `dependencies[].path` 落在 `crates/` 内的边全部列出,`kind` 为空即普通边):

```powershell
cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
# 对每个 package 的 dependencies 逐条过滤 path 属性并计数
```

## dev 边与例外

dev 依赖只服务测试与示例,不参与产品层位判定:

```text
nana-js-engine      → nana-js-engine        (dev,自测)
nana-js-v8          → nana-js-engine (dev), nana-ui, nana-ui-platform, nana-ui-vue, nana-ui-web-api (dev)
nana-text           → nana-ui-core          (dev,bundled-fonts)
nana-ui-scene       → nana-text, nana-ui-core, nana-ui-input (dev)
nana-packager       → nana-ui-platform      (dev)
nana-ui-dev         → nana-ui-platform, nana-ui-runtime, nana-ui-scene (dev)
```

例外的性质:`js-v8` 与 `dev` 的 dev 边「向上」指向产品 crate,是测试宿主行为;`scene` 的 dev 边连 core/text/input 只为构造测试场景。它们不进产品二进制。

工作区里还有 12 个非 crates/ 成员:examples(`component-gallery`、`vue-counter`、`vue-v8`、`vue-hosted-acceptance`、`reactive-sfc` 等 8 个)、`platform/android`(nana-android-host)、`tools/css-parity`(nana-css-parity)、fixtures(`package-fixture`、`runtime-host-fixture`)。它们的依赖全部指向产品 crate,是消费者,不参与分层。

## 给学习者的三句话

1. **想找权威实现,先看依赖表**:runtime 依赖 text 而不是反过来,所以文本权威在 text;`nana-ui` 谁都依赖,所以它是组装层不是权威层。
2. **反向依赖 = 第二套权威**。如果哪天 core 依赖了 runtime,合同层就开始知道树的细节,分层随之失效——这是 [合同与不变量](/learn/contracts-and-invariants) 里所有「不要做」的共同逻辑。
3. **一个 crate 的层级决定它能看到什么**。第 2 层的 css 看不见树;第 6 层的 scene 看不见 GPU。写代码前先确认你要的东西在你的层级可见。

## 接着读

- [合同与不变量](/learn/contracts-and-invariants):分层背后每条「为什么」
- [28 crate 速查表](/learn/coverage-matrix):四要素速查
- [架构合同(参考)](/reference/architecture):合同全文
