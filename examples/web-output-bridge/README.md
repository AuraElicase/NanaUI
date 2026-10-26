# NanaUI → Electron / WebGPU reference

This is a foreign host fixture. It does not introduce a WebView into NanaUI's
product shell. Windows x64, DX12, and Electron **44.9.1** are required. Metal,
Vulkan, ordinary browser pages, missing managed texture APIs, and unavailable
WebGPU capabilities return `unsupported`; there is no bitmap or CPU readback fallback.
Shader validation, lost devices, queue failures and unexpected exceptions are
reported as `error`, rather than being classified as missing platform evidence.

From the repository root on Windows:

```powershell
cargo build --release --locked -p nana-ui --example web-output-producer --features gpu,bundled-fonts,native-export,wgpu-interop
cd examples/web-output-bridge
npm ci --ignore-scripts=false
npm run check
npm test
npm start -- --producer ../../target/release/examples/web-output-producer.exe --frames 240 --output bridge-report.json
```

The fixture displays an actual NanaUI retained scene. The headless Rust process
owns one GPU context and painter. Electron owns its separate foreign GPU
consumer; the framework does not create another UI device or tree. A slow
browser defers export while the source UI continues producing on its own clock.
Each browser requires its own producer process/transport pool; native tokens
are never cloned across consumers.

## Ready and release contract

1. `ExternalSurface` supplies the latest completed revision. The existing
   `NativeExportPool` records one GPU copy and its ready-fence signal. The source
   lease is retained through that copy's submission.
2. Electron's public texture-import API has no NanaUI/DX12 fence argument. The
   producer therefore **queries** the ready fence without a CPU/GPU blocking
   wait, then duplicates the slot's NT handle into Electron's main process.
   `ready_poll_latency_ms` measures copy submission to observed readiness,
   including GPU completion and the 1 ms polling schedule. It is not GPU time.
3. Main calls `sharedTexture.importSharedTexture`, then acknowledges import.
   Electron makes its own NT-handle duplicate synchronously; Rust closes its
   temporary handle in the Electron process using `DuplicateHandle` with
   `DUPLICATE_CLOSE_SOURCE`, **only after the validated import acknowledgement**.
   Chromium owns and closes its separate duplicate. Without that acknowledgement,
   Rust leaves the temporary remote handle open: Electron may not yet have read
   the texture message, and early close could recycle its numeric HANDLE. At
   most one such unacknowledged handle exists; the stopped reference host exits,
   and Windows reclaims its process handle table. Producer failure never closes
   an unacknowledged handle that Electron may still import.
4. `sendSharedTexture` transfers the GPU-backed SharedImage to the renderer.
   `getVideoFrame` is consumed with WebGPU `importExternalTexture`; the fixture
   submits one textured draw. No Canvas2D, `copyTo`, mapped buffer, screenshot,
   bitmap or CPU pixel path is used.
5. The renderer asynchronously awaits its queue's completion, closes its
   `VideoFrame`, and releases its imported reference. Main releases its reference
   after the transfer promise. A native release receipt is sent only after
   **both** renderer GPU completion and main's `allReferencesReleased` callback.
   Renderer IPC alone cannot authorize reuse.
6. Rust validates frame, pool, device and resource generations. Only that
   receipt drops the unaccepted native token, which signals release on the
   original producer queue after ready. The pool cannot overwrite an exported
   slot before this point. Disconnect, device loss, invalid/duplicate receipts
   or timeout stop output and suppress automatic release of unconfirmed tokens;
   the isolated producer exits without reusing their allocation. Imported
   Chromium objects retain their own native-resource references during teardown.

Only opaque BGRA8 SDR/sRGB is supported by this fixture. Transparency, HDR,
resize and device replacement need separate explicit negotiation; they are not
silently converted or presented as validated here. Adapter mismatch can reject
native import; it never selects a CPU fallback.

`--frames 240` first completes 30 real imported, drawn and retired warmup
frames, then measures 240 more. Native receipt IDs and source revisions continue
monotonically across all 270 frames. The producer starts measurement only after
the 30th foreign release is observed on its real release fence. Setup allocation
is reported in `setup_work`, warmup in `warmup_output_work` / `warmup_proof`,
and steady work in `output_work` / `proof`. Thus `frames` and proof counts are
240, while `warmup_frames` is 30 and `total_frames` / `total_proof` are 270.

`consumer_record_cpu_ms` contains exactly one CPU sample per measured draw:
WebGPU texture import, binding/command encoding and `queue.submit`, timed with
`performance.now()`. It excludes the asynchronous GPU completion await and is
separate from producer `timing_ms`. Warmup CPU samples and consumer work are
reported separately. GPU execution time remains unobserved.

The final JSON includes actual imported/displayed/retired counts, source GPU
copies and bytes, submissions, deferrals and CPU call quantiles. Consumer WebGPU
draw/submission counts are separate from producer work. Chromium's internal GPU
copies, GPU timestamps and driver memory remain `null` when unobserved. A
successful syntax or cross-compilation check is not an Electron/GPU runtime pass.
The Node behavior tests exercise the same lifecycle and failure classification used by main:
proof ordering, duplicate and stale generations, failed transfer and disconnect,
and the actual 270-frame coordinator/measurement segmentation. Missing consumer
CPU samples fail instead of being filled with zero.
They provide protocol evidence, not Chromium/GPU runtime evidence.
No historical Release timing baseline is inferred.

## Official API evidence

- [Pinned managed API](https://github.com/electron/electron/blob/v44.9.1/docs/api/shared-texture.md):
  import is main-process only; the final callback covers all process references;
  transfer completion alone is not consumption completion.
- [Pinned implementation](https://github.com/electron/electron/blob/v44.9.1/shell/common/api/electron_api_shared_texture.cc):
  Windows imports duplicate the NT handle before putting it in a scoped DXGI
  handle; `CreateSharedImage` includes WebGPU usage; `getVideoFrame` wraps that
  SharedImage and records GPU release sync tokens.
- [Pinned imported texture contract](https://github.com/electron/electron/blob/v44.9.1/docs/api/structures/shared-texture-imported.md):
  closing a VideoFrame leaves underlying resources alive through GPU completion.
- [Shared-handle transport and ownership design](https://github.com/electron/electron/blob/v44.9.1/shell/common/api/shared_texture/README.md).
- [WebGPU queue completion](https://gpuweb.github.io/types/interfaces/GPUQueue.html#onSubmittedWorkDone).
- [Electron stable releases](https://releases.electronjs.org/release?channel=stable).
