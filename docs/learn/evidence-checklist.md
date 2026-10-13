---
title: 证据清单:关键架构断言怎么核
---

# 证据清单:关键架构断言怎么核

本页把学习者板块里反复出现的**架构断言**落成"断言 → 只读核对方式 → 当前读数"三列。所有命令在仓库根目录可复跑,不修改任何文件;读数日期为本板块编写时的工作区状态。

## A. 分层与依赖

### A1. 依赖沿 core → runtime → scene → nana-ui 单向、无环

**为什么重要**:反向边一旦出现,合同层开始知道树的细节,整个分层失效(见[合同与不变量](/learn/contracts-and-invariants))。

```powershell
# 工作区内普通边全量(cargo metadata 本身解析成功即证明无环:cargo 拒绝环形图)
cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
# 读法:对每个 package 的 dependencies[],凡 path 落在 crates/ 内且 kind 为空(普通)的即一条边
```

当前读数:**crates/ 内 77 条普通边(28 个成员间 76 条 + `nana-ui → nana-icons-tabler` 1 条),全部前向**。核心链:core 只依赖 nana-diagnostics;runtime 依赖 {core,text,charts,input,view-macros,view-schema,diagnostics};scene 依赖 {runtime,core,text(可选 benchmark),charts,diagnostics};nana-ui 依赖 {scene,runtime,core,…}。无一条低层指回高层的边。dev 边例外 6 组(scene、text、js-v8、js-engine 自测、packager、ui-dev),全部测试宿主性质,不进产品二进制——完整清单见[模块与分层](/learn/modules-and-layers)。

### A2. 四个 UI 主 crate 都不依赖 CSS 引擎

```powershell
rg "nana-ui-css" crates/nana-ui-core/Cargo.toml crates/nana-ui-runtime/Cargo.toml crates/nana-ui-scene/Cargo.toml crates/nana-ui/Cargo.toml   # 0 命中
```

读数:0 命中。CSS 引擎的消费者只有 view-codegen(构建期)与 nana-ui-vue(运行期),见 B1。

## B. 唯一实现清单

### B1. CSS 解析/级联只在 nana-ui-css

```powershell
cargo tree -p nana-ui-css --depth 1          # css -> core + serde_json,无树依赖
rg -i "selector|cascade" crates/nana-ui-runtime/src -g '!tests'   # 命中均为注释/文档/ICU 名词
rg "nana_ui_css" crates/*/src                # 仅 view-codegen(构建期)与 vue(运行期)
```

读数:runtime 模块文档原话——"No CSS is parsed and no selector is matched at run time, and this crate has no CSS in it"(`crates/nana-ui-runtime/src/view/style.rs:8-9`);core 无样式表解析器/选择器匹配/级联实现,CSS 字样落点全为**单值关键字映射与表达式 AST**:`box_layout.rs:1516/:1587/:1627/:1648/:1698` 五个 `parse(&str)` 解析 mix-blend-mode 等 CSS 关键字、`geometry.rs:57` `ContentFit::from_object_fit`(object-fit 关键字)、`box_layout/calc.rs:1` CSS `calc()` 表达式 AST、`style_model.rs:341` `from_css_token_name`(token 名映射);scene 仅 benchmark 注释;nana-ui 0 命中。

### B2. 输入路由唯一消费方是 runtime 的 AppContext::route_input

```powershell
cargo tree --workspace -i nana-ui-input --edges normal   # 普通边消费方:runtime + platform(再导出)
rg "pub fn route_input" crates                           # 唯一实现:crates/nana-ui-runtime/src/framework/input.rs:397
rg "route_input|route_shaped" crates -g '!*tests*'       # 各宿主全部汇入 context.route_input
```

读数:`route_input` 唯一实现在 runtime(impl AppContext,`crates/nana-ui-runtime/src/framework/input.rs:397`);vue 的 `route_input`(`crates/nana-ui-vue/src/host/input_projection.rs:386`)是投影包装,内部 `:430` 调 `context.route_input`;`hosted_adapter.rs:468/:1813` 同途;devtools 走 `route_shaped`(`crates/nana-ui-devtools/src/agent/runtime.rs:123`)→ headless 内部 `crates/nana-ui-runtime/src/framework/input/headless.rs:126` 仍是 `context.route_input`。无绕行路径。

### B3. vue/JS 宿主不建第二棵树、第二个 painter

```powershell
rg "PendingHostOps" crates/nana-ui-vue/src/tree.rs   # :220,宿主操作队列就是 runtime 的 UiMutation 队列
rg "SceneWgpuPainter|scene_paint" crates/nana-ui-vue/src   # 仅文档引用,无自绘管线
```

读数:`crates/nana-ui-vue/src/tree.rs:220` 的 PendingHostOps 类型即 runtime 的 UiMutation 队列;`tree.rs:206-218` VueRuntime 对 runtime Deref;`crates/nana-ui-vue/src/hosted_adapter.rs:966`(VueRuntimeProgram 文档)明言 "enters `run_runtime` / `SceneWgpuPainter`"——进入 nana-ui 的 painter 而非自建;`bridge.rs:9-19` MessageBridge 是投影桥接层。

### B4. 文本引擎唯一、GPU 后端唯一

```powershell
rg "cosmic_text|cryoglyph" crates/nana-text/src     # 命中见读数(check-engine-boundary.py 守门)
rg "nana_gpu::__framework" crates    # __framework 只被 gpu 系 crate 自身源码使用
```

读数:实跑命中 1 处——`crates/nana-text/src/lib.rs:25` 是陈述该边界本身的模块文档("No `cosmic_text` or `cryoglyph` identifier appears under `src/`"),非代码引用;产品依赖图由 `scripts/check-engine-boundary.py` 守门。文本侧金标准来自已删除的 cosmic 参考实现(#99),原生引擎对着金样核对,把两套候选引擎挡在产品依赖图外;GPU 侧 WGPU 唯一后端,公共签名里的 `wgpu::*` 只能经 `wgpu-interop` 出现在五个允许的 crate(`scripts/check-engine-boundary.py` 强制)。

## C. 帧路径不变量

| 断言 | 位置 | 核对 |
| --- | --- | --- |
| PendingHostOps 在帧边界提交 | `crates/nana-ui-scene/src/runtime_document.rs:109`(flush) | 读 flush:有界 8-pass,布局/提取/命中同代(`:28` layout_generation) |
| 滚动不写 Runtime 布局盒 | `crates/nana-ui-runtime/src/world/mutation.rs:2758`(scroll_to_clamped) | 只写 scroll_offset,LayoutBox 不动;LayoutBoxStore 是投影 |
| 单一 Device/Queue,跨代拒绝 | `crates/nana-gpu/src/context.rs:19`(DeviceGeneration)、`:715`(check_device) | 丢失后粘性首报,旧代资源访问被拒 |
| 产品无 CPU 回读路径 | `crates/nana-ui-devtools/src/offscreen.rs`(agent 会话) | 回读只存在于 devtools agent;`run_runtime` 的 present 路径不经过它 |
| 帧路径零格式化/零 I/O | `crates/nana-diagnostics/src/framework.rs:2` | event!/metric! 静态描述符,IDs append-only |

## D. 帧流程锚点(一条帧的 35 步全序)

一帧从输入到像素的完整顺序表在 [GPU 与帧](/learn/gpu-frame)(present.rs 35 步);四条端到端数据流(状态→像素、点击→回调、文字→像素、GPU 画面挂树)在 [数据流](/learn/flows)。本页不重复,只给入口锚点:

```powershell
rg "fn redraw" crates/nana-ui/src/scene_host/present.rs   # 一帧全序的函数体(pub(super) fn redraw,:114)
```

## E. 与代码一致性的复核方式

第三方复核建议按本页顺序:①跑 A1/A2 两条命令(秒级);②对 B1-B3 各跑一条 grep;③抽验 C 表任一锚点(打开文件跳到行号)。若任何读数与代码不符,以代码为准,并回到 [合同与不变量](/learn/contracts-and-invariants) 判断是文档过时还是合同被破坏。

## 接着读

- [合同与不变量](/learn/contracts-and-invariants):九条合同与核查读数;
- [模块与分层](/learn/modules-and-layers):77 条边全量清单与 dev 例外;
- [28 crate 速查表](/learn/coverage-matrix):每 crate 四件套与主章链接。
