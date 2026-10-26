# Fetch

网络权限在宿主，不在界面。默认一个源都不放行。你先列出允许的 origin，样式里的远程图片才取得到。

```rust
use nana_ui::{ApplicationWindow, FetchPolicy, NativeFetchHost, shared_fetch_host};

let policy = FetchPolicy::default().with_allowed_origin("https://cdn.example.com")?;
let mut window = ApplicationWindow::default();
window.fetch_host = Some(shared_fetch_host(NativeFetchHost::new(policy)));
```

`FetchPolicy`、`NativeFetchHost` 和 `shared_fetch_host` 在开启 `gpu` 时从 `nana_ui` 再导出，定义在 `nana_ui_platform`。

## 白名单和上限

源用 `allow_origin` / `with_allowed_origin` 登记，格式 `scheme://host[:port]`。带路径、query 或 fragment 报 `InvalidRequest`。不是 http / https 报 `Policy`。放行 `https://example.com` 就放行它下面的所有路径。`https://api.example.com` 是另一个 origin，要单独登记。localhost 也要单独放行。

| 字段 | 默认 |
| --- | --- |
| `timeout` | 30 秒，跨重定向共享 |
| `max_request_bytes` | 16 MiB（`16 * 1024 * 1024`） |
| `max_response_bytes` | 16 MiB，流式按累计字节算 |
| `max_redirects` | 5，超过报 `Redirect` |

`NativeFetchHost` 是阻塞的 `ureq`，必须跑在 UI 线程之外；painter 在自己的取图线程上调用它。每一跳都重新核对白名单。跨源时摘掉 `authorization` 和 `proxy-authorization`。303，以及 POST 的 301 / 302，转成 GET，并清空正文和 `content-length` / `content-type`。响应里的 `set-cookie` 被丢弃，没有 cookie jar。

错误按 `FetchErrorKind`：`Policy`、`InvalidRequest`、`Network`、`Timeout`、`RequestTooLarge`、`ResponseTooLarge`、`Redirect`、`Cancelled`、`Unsupported`。

## 哪些资源走这条路

`url()` 图片、`<img src>`、`mask-image`、`border-image` 共用同一个 `FetchHost`、同一份 `FetchPolicy`：同样的白名单，同样逐跳复核重定向，同样在跨源时摘掉授权头。

没装 host，远程图就不取。`data:`、`file:` 和 jail 里的相对路径不受影响。host 按文档分开。多个窗口共用一个 painter 时，每个文档的图片只经过它自己的 host，缓存也按 host 分开。

窗口的默认是 `None`，远程图会被拒绝。给窗口填 `ApplicationWindow.fetch_host`，或实现 `RuntimeProgram::resource_fetch_host`。自己持有 painter 时，绘制某个文档之前用 `SceneWgpuPainter::set_resource_fetch_host` 把该文档的 host 交过去。

取消发生在 painter 析构，以及图片连续 120 帧没有引用的时候。结束的是网络等待，不是已经开始的解码。`@font-face` 不走网络，只认 `local()`、`data:` 和 jail 内的本机文件。

## 自己实现 host

宿主接口是 `FetchHost`：实现 `fetch` 和 `policy()`。`policy()` 声明这个 host 自己的白名单和上限。

宿主合同在 [应用 API](../../reference/application-api.md#fetch-宿主)。

## 接下来

<div class="next-steps">
  <a class="next-step" href="/guide/components/async">
    <p class="next-step-link">异步</p>
    <p class="next-step-caption">Rust 视图里的 resource 跑在 UI 线程的执行器上。</p>
  </a>
  <a class="next-step" href="/guide/scaling/packaging">
    <p class="next-step-link">打包</p>
    <p class="next-step-caption">样式里的 nana://res 从资源包读取。</p>
  </a>
  <a class="next-step" href="/guide/scaling/accessibility">
    <p class="next-step-link">无障碍</p>
    <p class="next-step-caption">名字、隐藏层级和窗口根。</p>
  </a>
</div>
