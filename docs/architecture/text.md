# 文本测量、整形与绘制

NanaUI 的文本测量、OpenType 整形与保留文本布局由单一权威 `nana-text` 负责；字形栅格化、图集打包与 GPU 绘制由 `nana-ui` 的 `NanaRenderer::text`（`scene_paint/text/`）完成。测量与绘制共享同一个进程级引擎实例（`text_engine.rs`），`RuntimeDocument::flush` 调用的 `NanaTextShaper` 正是它的测量入口。

```text
UiWorld 中的文本节点
    │  NanaTextShaper（flush 阶段）
    ▼
nana-text           字体匹配 → OpenType 整形（ShapedText）→ 行排版（TextLayout）
    │  TextLayoutId 世代句柄留在节点上
    ▼
UiScene             ScenePrimitiveKind::Text（直接携带保留的 TextLayout）
    │
    ▼
NanaRenderer::text  字形栅格化、多页图集（Atlas）上传，按文档顺序画入当前帧
```

关于缓存键、中日文标点挤压与评测语料的完整细节，请查阅维护者文档 [文本引擎](../reference/text-engine.md) 与 [架构学习：文本引擎](/learn/text-engine)。

## 引擎职责与三段式流水线

`nana-text` 拥有文本中间表示（IR）、代际句柄（`{ index, generation }`，槽位回收后递增代际，旧句柄直接被拒）以及「何时重做整形、如何断行排版」的调度逻辑；底层标准算法（HarfRust OpenType 整形、skrifa 字表读取、Unicode BiDi 与 UAX #14 断行表）封装在私有模块内，排版枚举（变体轴、字距、断行、对齐、方向、书写模式、行高）直接复用 `nana-ui-core` 的定义。

一段文本从样式到布局依次经过严格解耦的三个阶段：

```text
1. 字体匹配（FontSystem）  ：TextStyle → FontQuery → FontSelection → FontAssignment
2. 文本整形（Shaper）      ：按字素簇 / Script / BiDi 分段 → HarfRust 整形 → 不可变 ShapedText
3. 段落排版（Layouter）    ：ShapedText + TextConstraints → 单行快路径或 UAX #14 折行 → 不可变 TextLayout
```

- **阶段一：字体选择与覆盖率回退**。字体层将样式收敛为一次查询，选出主字体与回退链，并按 `cmap` 覆盖率将文本区间分配到具体的字体面（face）。字体不存在的变体轴会被安全跳过，不会谎报或改写为 `wght`；若覆盖率选中的字体面在整形后仍产出 `.notdef` 缺字字形，整形器会自动换候选字体重试。
- **阶段二：OpenType 整形（Shaping）**。将字符序列转换为不可变的 `ShapedRun`，此阶段不折行、也不计算光标位置。分段严格遵守字素簇（grapheme cluster）、脚本与 BiDi 边界（样式跨度边界不会切碎同一个字素簇；段落方向由样式的 `direction` 决定，不靠首字符猜测）。**颜色、不透明度、几何变换、最大宽度与折行方式均不进入整形缓存键**——相同文字与塑形样式的标签在全进程内共享同一份 `ShapedText`。
- **阶段三：行排版（Layout）**。将 `ShapedText` 与容器宽度/行数约束结合，产出不可变的 `TextLayout`。单行、不换行且无高度限制的普通标签走单行快路径；多行段落按已整形的精确字形步进（advance）在 UAX #14 断点上折行（绝不按字符数粗略估算）。**改变容器宽度、折行方式、行高或对齐方式只需重跑第 3 步排版，完全不触发重新整形**。

## 保留句柄与分级失效

Runtime 节点（`TextNodeState`）只保存逻辑文本、修订号与 `TextLayoutId` 句柄，实际的整形与排版缓存由进程级引擎持有。当节点发生变更时，Runtime 在写入处按实际受影响的层级精确分流：

| 发生了什么变更 | 触发的文本工作 |
| --- | --- |
| 文本内容、字体族、字号、字重、字距、变体轴（含富文本对应 span） | 重新整形（Shaping）+ 重新排版（Layout） |
| 容器宽度、折行策略、行高、对齐方式、内联对象（贴纸/图片）尺寸 | **仅重新排版**（复用已有 `ShapedText`） |
| 纯文本颜色、内联对象显示内容 | **零文本引擎工作**（不整形、不排版，仅重绘） |
| 富文本 span 的颜色、描边、阴影、装饰线 | **零文本引擎工作**（画笔仅重建对应区间的字形绘制实例） |
| 节点 `transform` / `opacity`、逐字揭示特效（`GLYPH_PRESENTATION`） | **完全不触碰文本层**（仅在合成器求值） |

当节点的三个修订号与字体代际均未变化时，在复制字符串或查询缓存之前就会以 $O(1)$ 整数比较直接跳过，开销与文本长度无关。

## 可编辑文本与输入法组字（IME Preedit）

静态标签不分配编辑会话。只有可编辑文本控件才挂载编辑层：

- **按自然段（换行符）切分布局**：每段各自持有一份 `TextLayout`，输入一个字符只重排所在的那一段；单纯移动光标或改变选区范围既不重新整形，也不重新排版。
- **组字（Preedit）是瞬态覆盖层**：IME 组字期间，已提交的底层文本保持不动，屏幕按「已提交文本 + preedit 覆盖」排版显示；失焦时自动丢弃未完成的 preedit，而不是先提交进业务文本再撤销。光标、指针命中与选区矩形全部直接从保留的 `TextLayout` 推导。

## 绘制阶段：直接消费保留布局

`NanaRenderer::text` 直接使用 Runtime 留在节点上的 `TextLayout` 句柄进行绘制，不再从场景描述里重新排版一遍（仅无句柄的内建提示或对齐盒被改写的特殊装饰节点才由画笔兜底排版）。

字形栅格缓存的键不包含颜色、不透明度与场景变换，因此同一个字在不同颜色下共享同一张图集位图；当布局、亚像素相位与字体代际均未变化时，画笔直接复用已生成的字形实例缓冲。若发生 GPU 设备丢失重建，旧图集随旧 painter 释放，新设备首帧会自动将用到的字形重新栅格化上传，而段落布局本身仍直接复用 Runtime 保留的 `TextLayout`。

## 接着读

<div class="next-steps">
  <a class="next-step" href="/architecture/window">
    <p class="next-step-link">窗口</p>
    <p class="next-step-caption">原生窗口、自绘标题栏与系统材质边界。</p>
  </a>
  <a class="next-step" href="/reference/text-engine">
    <p class="next-step-link">文本引擎参考合同</p>
    <p class="next-step-caption">字体回退、整形缓存、中日文排版与可编辑路径的完整规范。</p>
  </a>
  <a class="next-step" href="/architecture/frame">
    <p class="next-step-link">一帧</p>
    <p class="next-step-caption">文本在 flush 事务中何时测量、如何与布局反馈收敛。</p>
  </a>
</div>
