---
title: 28 crate 覆盖矩阵
---

# 28 crate 覆盖矩阵:每个 crate 在哪一章、查什么

本页是[学习者板块](/learn/)的**可清点底账**:工作区 `crates/` 下全部 **28** 个 crate,每个 crate 在至少一章中拥有四件套——**职责**(它是什么)、**依赖方向**(它依赖谁/被谁依赖)、**关键类型/入口**(从哪个类型或函数开始读)、**功能说明**(它做什么、不做什么)。核对方法每行都给出:锚点(文件:行)或命令,全部只读、可复跑。

```powershell
# 清点 crate 数(应输出 28)
(Get-ChildItem crates -Directory).Count
```

## 主线 crate(产品路径 4 + 文本 1)

| Crate | 职责一句话 | 主章 | 依赖方向 | 关键入口 | 核对 |
| --- | --- | --- | --- | --- | --- |
| **nana-ui-core** | 全框架词汇:样式/几何/主题/动效/工作区的值类型 | [核心合同](/learn/core-contracts) | 入:nana-diagnostics;出:几乎全部上层 | `LayoutStyle`(box_layout.rs:3292)、`SemanticColorRole`(style_model.rs:257) | `cargo tree -p nana-ui-core --depth 1` |
| **nana-ui-runtime** | 唯一权威树 UiWorld:变更/布局/输入/焦点/IME/无障碍/Shell | [保留树](/learn/runtime-tree) · [布局引擎](/learn/runtime-layout) · [输入链路](/learn/runtime-input) · [工作台](/learn/workspace-shell-dock) | 入:view-schema、view-macros、core、text、charts、input、diagnostics;出:nana-ui、scene、vue、devtools | `UiWorld`(world.rs:628)、`UiMutation`(mutation.rs:15)、`AppContext`(framework.rs:1072) | `rg "pub struct UiWorld" crates/nana-ui-runtime/src/world.rs` |
| **nana-ui-scene** | 中立绘制图 UiScene 与帧程序 FramePlan,不碰 GPU | [Scene 提取](/learn/scene-extraction) | 入:runtime、core;**无** gpu/css 依赖 | `UiScene`(scene.rs:1357)、`apply_delta`(scene.rs:1672)、`RuntimeDocument::flush`(runtime_document.rs:109) | `cargo tree -p nana-ui-scene --depth 1` |
| **nana-ui** | 宿主适配:SceneWgpuPainter、宿主一帧、窗口服务、无障碍后端 | [GPU 与帧](/learn/gpu-frame) · [窗口与平台](/learn/window-platform) | 入:gpu、window、platform、scene、runtime、frame-exchange、svg-raster、package、icons 等 | `SceneWgpuPainter`(scene_paint/mod.rs:381)、`run_runtime_scene`(scene_host.rs:99) | `rg "pub struct SceneWgpuPainter" crates/nana-ui/src/scene_paint/mod.rs` |
| **nana-text** | 唯一文本引擎:测量/整形/断行/编辑会话/金样对齐 | [文本引擎](/learn/text-engine) | 入:nana-ui-core、serde、fontdb、skrifa、harfrust、unicode-*;出:runtime、nana-ui、web-api、scene(可选 benchmark) | `TextEngine`(engine.rs:25)、`Shaper`(shaping/shaper.rs:27)、`Layouter`(layout/engine.rs:157) | `cargo test -p nana-text` |

## 引擎与合同 crate(CSS / 输入 / GPU / 帧交换)

| Crate | 职责一句话 | 主章 | 依赖方向 | 关键入口 | 核对 |
| --- | --- | --- | --- | --- | --- |
| **nana-ui-css** | 唯一 CSS 词法/解析/级联,产出 LayoutStyle 补丁 | [CSS 引擎](/learn/css-engine) | 入:core、serde_json;出:view-codegen(构建期)、vue(运行时) | `parse_stylesheet`(css_cascade.rs:658)、`rebuild_layout_style`(:1606) | `cargo tree -p nana-ui-css --depth 1` |
| **nana-ui-input** | 规范输入事件/端点/时序器/HostServices 合同 | [输入链路](/learn/runtime-input) | 入:core、diagnostics;出:runtime(路由消费)、platform(再导出) | `CanonicalInputEvent`(canonical.rs:44)、`InputEndpoint`(:410)、`HostServices`(host_services.rs:97) | `cargo tree --workspace -i nana-ui-input --edges normal` |
| **nana-gpu** | GPU 后端合同:GpuContext/FrameContext/GpuTexture、代际与提交守卫 | [GPU 与帧](/learn/gpu-frame) | 入:diagnostics(dev:wgpu);出:nana-ui、vue、devtools、frame-exchange(经 __framework) | `GpuContext`(context.rs:358)、`FrameContext`(frame.rs:38) | `rg "pub struct GpuContext" crates/nana-gpu/src/context.rs` |
| **nana-frame-exchange** | 有界最新帧交换,不拥有 UI 设备与提交 | [GPU 与帧](/learn/gpu-frame) | 入:gpu(经 __framework);出:nana-ui(FrameBinding) | `FrameExchange`(lib.rs:493)、`FrameLease`(:370) | `rg "copy_texture_to_texture" crates/nana-frame-exchange/src/lib.rs` |

## 窗口与平台 crate

| Crate | 职责一句话 | 主章 | 依赖方向 | 关键入口 | 核对 |
| --- | --- | --- | --- | --- | --- |
| **nana-ui-platform** | 平台中立合同:窗口/输入转发/fetch/ws/剪贴板/持久化/路径 | [窗口与平台](/learn/window-platform) | 入:core、input、diagnostics、ureq、url、arboard 等;出:runtime 之上各宿主 | `WindowDescriptor`(window.rs:429)、`WindowEvent`(:114)、`WindowCommand`(:684) | `cargo tree -p nana-ui-platform --depth 1` |
| **nana-window** | 原生执行:句柄、材质、无框 chrome、缩放、全屏、underlay、WebSurface | [窗口与平台](/learn/window-platform) | 入:nana-ui-core、window-vibrancy、url(dev:png);被 nana-ui 借用 | `apply_system_material`(material.rs:176)、`prepare_client_chrome`(chrome.rs:32) | `rg "pub fn apply_system_material" crates/nana-window/src/material.rs` |

## 作者工具链 crate(7)

| Crate | 职责一句话 | 主章 | 依赖方向 | 关键入口 | 核对 |
| --- | --- | --- | --- | --- | --- |
| **nana-ui-view-schema** | 内建控件唯一描述表 for_each_control! | [作者工具链](/learn/toolchain) | 入:零依赖;出:runtime 与 codegen 同时消费 | `for_each_control!`(lib.rs:49,236 行一整张表) | 读 `crates/nana-ui-view-schema/src/lib.rs` 全文 |
| **nana-ui-view-macros** | view!/css!/stylesheet! 过程宏入口 | [作者工具链](/learn/toolchain) | 入:view-codegen;出:runtime(view-macro feature) | `view!`(lib.rs:16)、`css!`(:41)、`stylesheet!`(:76) | `rg "^pub fn" crates/nana-ui-view-macros/src/lib.rs` |
| **nana-ui-view-codegen** | 模板→声明式视图函数;构建时调 CSS 引擎 | [作者工具链](/learn/toolchain) | 入:core、css、view-schema | `expand`(lib.rs:287)、`compile_styles`(style.rs:986) | `rg "use nana_ui_css" crates/nana-ui-view-codegen/src/style.rs` |
| **nana-ui-sfc** | .vue 单文件组件构建期编译器(Rust script) | [作者工具链](/learn/toolchain) | 入:view-codegen(+syn/quote 等);产物指向 runtime | `Compiler`(lib.rs:115)、`component()`(lib.rs:413) | `rg "pub struct Compiler" crates/nana-ui-sfc/src/lib.rs` |
| **nana-js-engine** | JsEngine trait 与 HostApiRegistry 合同(注入缝) | [作者工具链](/learn/toolchain) | 入:零普通依赖;出:vue、js-v8、web-api | `trait JsEngine`(lib.rs:1254)、`HostApiRegistry`(:1068) | `rg "pub trait JsEngine" crates/nana-js-engine/src/lib.rs` |
| **nana-js-v8** | V8 对 JsEngine 合同的实现 | [作者工具链](/learn/toolchain) | 入:js-engine(engine feature 才链 v8 152.2.0) | `V8Engine`(engine.rs,4384 行) | `rg "engine" crates/nana-js-v8/Cargo.toml` |
| **nana-ui-web-api** | JS 侧 window/document/fetch/计时器等宿主 API | [作者工具链](/learn/toolchain) | 入:js-engine、core、platform、text、svg-raster | `WebApiState`(lib.rs:75)、`register_web_api_host_ops`(:508) | `rg "pub fn register_web_api_host_ops" crates/nana-ui-web-api/src/lib.rs` |

## 外围支撑 crate(9)

| Crate | 职责一句话 | 主章 | 依赖方向 | 关键入口 | 核对 |
| --- | --- | --- | --- | --- | --- |
| **nana-ui-charts** | 图表模型:option/scales/layout/hit/GPU mark 数组 | [外围支撑](/learn/supporting-crates) | 入:仅 core + bytemuck;Chart 组件在 runtime,管线在 nana-ui | `ChartOption`(option.rs:105)、`ChartMarks`(marks.rs:203) | 读 `crates/nana-ui-charts/Cargo.toml` 的 [dependencies] |
| **nana-diagnostics** | event!/metric!/fault!/span! 宏与静态描述符 | [外围支撑](/learn/supporting-crates) | 入:零普通依赖;出:core 起整条产品线 | `event!`(lib.rs:83)、framework.rs 描述符表 | `rg "never renumber" crates/nana-diagnostics/src/framework.rs` |
| **nana-svg-raster** | CPU 侧 SVG 栅格化(resvg→pixmap) | [外围支撑](/learn/supporting-crates) | 入:resvg 0.48.1 等;出:web-api、nana-ui | `rasterize_document`(lib.rs:58) | `rg "pub fn rasterize" crates/nana-svg-raster/src/lib.rs` |
| **nana-app-icon** | 默认标记 + PNG/ICO/ICNS 编码 + PE 嵌入 | [外围支撑](/learn/supporting-crates) | 入:platform、image、flate2、embed-resource | `rasterize`(mark.rs:8)、feature embed/runtime | `rg "feature" crates/nana-app-icon/Cargo.toml` |
| **nana-icons-tabler** | 生成的 Tabler 图标常量表(383983 行) | [外围支撑](/learn/supporting-crates) | 入:core 的 icon 类型;非 workspace member,链接器裁剪 | `*_ICON: IconData` 常量族 | 读文件头 @generated 注释 |
| **nana-package** | 打包契约运行时读侧(.nrpack) | [外围支撑](/learn/supporting-crates) | 入:纯 Rust(blake3、纯 Rust zstd 解码);出:nana-ui 宿主 | `PackReader`(pack/reader.rs:47)、`ResourceClass`(pack/mod.rs:30) | 读 `crates/nana-package/src/lib.rs` 头文档 |
| **nana-packager** | 打包构建侧 CLI(六子命令) | [外围支撑](/learn/supporting-crates) | 入:链 C zstd encoder;不随应用发布 | `main.rs:32-38` 子命令表 | `rg "enum Command" crates/nana-packager/src/main.rs` |
| **nana-ui-dev** | 热重载公共底座(dev-only,compile_error! 守门) | [外围支撑](/learn/supporting-crates) | 入:js-engine、ui、core、devtools、sfc、vue、notify 等;feature vue/templates | `ReloadRequest`(lib.rs:58)、compile_error(lib.rs:22) | `rg "compile_error" crates/nana-ui-dev/src/lib.rs` |
| **nana-ui-devtools** | 有界诊断记录 + 无头 agent 会话 | [外围支撑](/learn/supporting-crates) | 入:core、platform、ui、scene、vue、js-engine、js-v8、gpu、wgpu(feature 嵌套) | `DevtoolsSession`(lib.rs:87)、`AgentSession`(agent/session.rs:38) | `rg "no product-facing UI" crates/nana-ui-devtools/src/lib.rs` |

## 宿主适配 crate(1)

| Crate | 职责一句话 | 主章 | 依赖方向 | 关键入口 | 核对 |
| --- | --- | --- | --- | --- | --- |
| **nana-ui-vue** | 把 Vue 声明与 JS host ops 接进同一 UiWorld,不建第二棵树 | [三种作者入口](/learn/authoring-paths) · [保留树](/learn/runtime-tree) | 入:core、css、runtime、scene、nana-ui、gpu、web-api、js-engine、svg-raster、app-icon(+wgpu) | `VueHost`(lib.rs:577)、`MessageBridge`(bridge.rs:88)、`register_dom_host_ops`(renderer.rs:43) | `rg "PendingHostOps" crates/nana-ui-vue/src/tree.rs`(宿主操作仍走 runtime 的 mutation 队列) |

## 覆盖统计与核对方法

- **28/28**:上表五行分组合计 5+4+2+7+9+1 = 28,每个 crate 一行、四件套齐。

**四件套的核对方式**:每章的「职责与边界」小节给职责与不做的事;「依赖与邻居」表给方向;「关键类型与数据」表给入口(全部 file:line 锚点);正文与代码地图给功能说明。第三方复核直接:①数 `(Get-ChildItem crates -Directory).Count` = 28;②对上表每行点开主章,确认锚点小节能定位到对应类型。

## 延伸阅读

- [模块与分层](/learn/modules-and-layers):28 crate 的层级总表与依赖边全量;
- [证据清单](/learn/evidence-checklist):关键架构断言与代码一致性的逐条核验读数;
- [章节地图](/learn/):按学习路径组织的全部页面。
