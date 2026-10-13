# 架构学习

这个板块写给**想读懂 NanaUI 这套框架本身**的人：它按模块(28 个 crate)和功能(布局、样式、文本、Scene、GPU、输入、窗口、工具链)把整个仓库拆开讲,并且下钻到各子系统的内部结构。

它与其他板块的分工:

| 板块 | 视角 |
| --- | --- |
| [指南](/guide/introduction) | 用 NanaUI 写应用 |
| [架构](/architecture/) | 应用作者需要的心智模型(一帧、实时画面、文本、窗口) |
| **架构学习(本板块)** | 面向架构学习者:全 crate 覆盖 + 子系统内部 + 数据流 |
| [参考](/reference/how-it-works) | 合同全文(crate 边界、消费 API、Runtime 与 Scene) |

## 一张图先立住

```text
Rust / Vue / JS 输入
    -> nana-ui-core        共享合同:Style Model、主题、几何、Workspace、运动
    -> nana-ui-runtime     UiWorld:保留树、控件、布局、输入、焦点、Shell、Dock
    -> nana-ui-scene       抽取出的绘制图(不依赖 WGPU)
    -> nana-ui             宿主适配器 + SceneWgpuPainter
```

产品路径只有这一条:`UiWorld → ExtractedNode → UiScene → SceneWgpuPainter`。支撑面(CSS 引擎、文本引擎、输入合同、平台与窗口、GPU 合同)各自只有一处权威实现,详见[模块与分层](/learn/modules-and-layers)与[合同与不变量](/learn/contracts-and-invariants)。

## 章节地图

### 全局

| 章 | 内容 |
| --- | --- |
| [模块与分层](/learn/modules-and-layers) | 28 个 crate 的层位分组、依赖图、分层规则与核查方法 |
| [合同与不变量](/learn/contracts-and-invariants) | 非协商合同清单:每条配「为什么」与违反后果 |
| [端到端数据流](/learn/flows) | 四条流:一帧从输入到像素、输入事件路由、文本渲染、GPU 内容挂树 |
| [怎么读这份代码](/learn/reading-the-code) | 入口文件地图、示例索引、调试手段 |

### 基础合同层

| 章 | 内容 |
| --- | --- |
| [core:共享合同](/learn/core-contracts) | nana-ui-core 的 Style Model、主题令牌、几何、WorkspaceModel、Motion IR |
| [CSS 引擎](/learn/css-engine) | nana-ui-css 的词法/解析/级联,运行时与构建时两个时机 |
| [文本引擎](/learn/text-engine) | nana-text 的测量→整形→保留文本布局三段 |

### Runtime 内部

| 章 | 内容 |
| --- | --- |
| [保留树:UiWorld](/learn/runtime-tree) | 树结构、组件与 ComponentRegistry、LayoutBoxStore 投影 |
| [布局引擎](/learn/runtime-layout) | 入参、算法组织、增量布局 |
| [输入链路](/learn/runtime-input) | nana-ui-input 合同、事件路由、焦点、IME、无障碍 |
| [Shell、Workspace 与 Dock](/learn/workspace-shell-dock) | 桌面壳结构、区域/分隔/折叠/Tab、逻辑状态与呈现分离 |

### 渲染与宿主面

| 章 | 内容 |
| --- | --- |
| [Scene 提取](/learn/scene-extraction) | nana-ui-scene 绘制图结构、提取过程、为何不依赖 WGPU |
| [GPU 与帧生命周期](/learn/gpu-frame) | nana-gpu 合同、SceneWgpuPainter、一帧的完整周期、nana-frame-exchange |
| [窗口与平台](/learn/window-platform) | nana-window 原生窗口/材质/chrome/缩放,nana-ui-platform 中立合同 |

### 作者入口与工具链

| 章 | 内容 |
| --- | --- |
| [三种作者入口](/learn/authoring-paths) | Rust `view!` 宏链、Vue/SFC 链、JS 宿主,如何汇到同一棵树 |
| [编译工具链](/learn/toolchain) | view-macros/view-schema/view-codegen、SFC 编译、JS 引擎面 |
| [支撑 crate 一览](/learn/supporting-crates) | charts、diagnostics、svg-raster、图标、打包、dev/devtools 及周边生态 |

### 附录

| 章 | 内容 |
| --- | --- |
| [28 crate 速查表](/learn/coverage-matrix) | 每个 crate 的职责/依赖方向/关键类型/归属章节四要素 |
| [断言核查清单](/learn/evidence-checklist) | 每条架构断言的核查命令、实际读数与结论 |

## 28 个 crate 的归属

| Crate | 归属章节 |
| --- | --- |
| nana-ui-core | [core:共享合同](/learn/core-contracts) |
| nana-ui-css | [CSS 引擎](/learn/css-engine) |
| nana-text | [文本引擎](/learn/text-engine) |
| nana-ui-runtime | [保留树](/learn/runtime-tree) · [布局引擎](/learn/runtime-layout) · [输入链路](/learn/runtime-input) · [Shell/Workspace/Dock](/learn/workspace-shell-dock) |
| nana-ui-input | [输入链路](/learn/runtime-input) |
| nana-ui-scene | [Scene 提取](/learn/scene-extraction) |
| nana-gpu | [GPU 与帧生命周期](/learn/gpu-frame) |
| nana-frame-exchange | [GPU 与帧生命周期](/learn/gpu-frame) |
| nana-ui | [GPU 与帧生命周期](/learn/gpu-frame) · [作者入口](/learn/authoring-paths) |
| nana-ui-platform | [窗口与平台](/learn/window-platform) |
| nana-window | [窗口与平台](/learn/window-platform) |
| nana-ui-vue | [作者入口](/learn/authoring-paths) |
| nana-js-engine | [编译工具链](/learn/toolchain) |
| nana-js-v8 | [编译工具链](/learn/toolchain) |
| nana-ui-web-api | [编译工具链](/learn/toolchain) |
| nana-ui-sfc | [编译工具链](/learn/toolchain) |
| nana-ui-view-macros | [编译工具链](/learn/toolchain) |
| nana-ui-view-schema | [编译工具链](/learn/toolchain) |
| nana-ui-view-codegen | [编译工具链](/learn/toolchain) |
| nana-ui-charts | [支撑 crate 一览](/learn/supporting-crates) |
| nana-diagnostics | [支撑 crate 一览](/learn/supporting-crates) |
| nana-svg-raster | [支撑 crate 一览](/learn/supporting-crates) |
| nana-app-icon | [支撑 crate 一览](/learn/supporting-crates) |
| nana-icons-tabler | [支撑 crate 一览](/learn/supporting-crates) |
| nana-package | [支撑 crate 一览](/learn/supporting-crates) |
| nana-packager | [支撑 crate 一览](/learn/supporting-crates) |
| nana-ui-dev | [支撑 crate 一览](/learn/supporting-crates) |
| nana-ui-devtools | [支撑 crate 一览](/learn/supporting-crates) |

## 三条阅读路径

- **从全图入手**:模块与分层 → 合同与不变量 → 挑感兴趣的子系统章 → 数据流。
- **从一帧入手**:数据流(一帧从输入到像素)→ 保留树 → Scene 提取 → GPU 与帧生命周期。
- **从某个 crate 入手**:[28 crate 速查表](/learn/coverage-matrix)找到它的职责与依赖方向,跳到归属章节,再沿数据流看它的上下游。
