# Windows GPU 共享纹理输入

`nana-gpu` 的 `native-import` feature 提供 `NativeImportPool`：外部 GPU 生产者写入共享 BGRA8 纹理，宿主在自己的 DX12 队列复制到可采样的私有纹理。没有 CPU 像素回读或上传；仍有一次显存内复制，因此不称为完全零拷贝。

## 生命周期与同步

1. 用现有 `GpuContext` 和像素尺寸创建 pool。它固定分配三个共享输入槽及一张私有输出，不创建第二个 WGPU device/queue。
2. `try_acquire` 非阻塞领取槽；池满时跳过本帧。target 提供适配器 LUID、借用 NT texture/fence handle、release/ready 值。生产者必须选择相同适配器。
3. 生产者 GPU Wait release，再写 BGRA8，离开共享目标并恢复 COMMON，Signal ready、Flush；成功排队后才可 `unsafe into_ready`。若写入后无法保证 Signal 执行，先 `pool.retire()`，禁止把槽重新开放给生产者。
4. `stage(&mut FrameContext, ready)` 在宿主 GPU Wait ready、复制、将共享源转回 COMMON；提交后 Signal release。丢弃 ready 或未提交的 frame 也在 GPU 时间线上归还槽，没有 CPU 等待。
5. 仅在 frame 已提交后发布输出。返回纹理是同队列顺序覆写的最新画面，不是不可变帧快照；需要不可变帧时应另接有租约的帧交换。默认采样视图为 sRGB，底层 BGRA8 UNORM 可供生产者声明的其他采样方式使用。

每次新载入、尺寸或设备代际变化都新建 pool；不能让新内容覆写仍归属旧播放身份的输出。先清理生产者的 native-handle 缓存，再释放旧 pool，防止 OS 重用句柄数值后误命中旧纹理。旧 target、ready 和已提交 GPU 操作保留自己的资源所有权。非 DX12、错误代际、退役池和跨 pool 帧均明确拒绝。

`HostTexture` / `GpuTextureView` 继续使用普通宿主纹理路径。框架桥不负责生产者 EGL/OpenGL 上下文、解码器、播放状态或应用回退策略。

## 验证

`cargo test -p nana-gpu --features native-import import::tests` 覆盖真实 D3D11 写入→DX12 接收像素、稳态资源复用、池满、丢弃、退役以及跨设备/跨池拒绝。依赖 Windows/DX12 原生设备；编译检查不能代替此类 GPU 验证。
