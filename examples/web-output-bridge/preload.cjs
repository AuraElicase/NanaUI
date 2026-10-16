'use strict';
const { ipcRenderer, sharedTexture } = require('electron');
const { UnsupportedCapability, bridgeFailure } = require('./failures.cjs');

window.addEventListener('DOMContentLoaded', async () => {
  try {
    if (!navigator.gpu) throw new UnsupportedCapability('webgpu-api', 'WebGPU is unavailable; no CPU fallback');
    const adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
    if (!adapter) throw new UnsupportedCapability('webgpu-adapter', 'No WebGPU adapter');
    const device = await adapter.requestDevice();
    device.addEventListener('uncapturederror', event => ipcRenderer.send('bridge-error', bridgeFailure(event.error)));
    const canvas = document.querySelector('canvas');
    const context = canvas.getContext('webgpu');
    if (!context) throw new UnsupportedCapability('webgpu-canvas', 'No WebGPU canvas');
    const format = navigator.gpu.getPreferredCanvasFormat();
    context.configure({ device, format, alphaMode: 'opaque' });
    const module = device.createShaderModule({ code: `
      struct Vertex { @builtin(position) position: vec4f, @location(0) uv: vec2f };
      @vertex fn vertex(@builtin(vertex_index) i: u32) -> Vertex {
        let corner = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
        var v: Vertex; v.position = vec4f(corner * 2.0 - 1.0, 0.0, 1.0);
        v.uv = vec2f(corner.x, 1.0 - corner.y); return v;
      }
      @group(0) @binding(0) var image: texture_external;
      @group(0) @binding(1) var imageSampler: sampler;
      @fragment fn fragment(v: Vertex) -> @location(0) vec4f {
        return textureSampleBaseClampToEdge(image, imageSampler, v.uv);
      }` });
    const pipeline = device.createRenderPipeline({ layout: 'auto', vertex: { module, entryPoint: 'vertex' },
      fragment: { module, entryPoint: 'fragment', targets: [{ format }] }, primitive: { topology: 'triangle-list' } });
    const sampler = device.createSampler({ minFilter: 'linear', magFilter: 'linear' });
    device.lost.then(info => ipcRenderer.send('bridge-error', bridgeFailure(new Error(`WebGPU device lost: ${info.reason}`))));
    sharedTexture.setSharedTextureReceiver(async ({ importedSharedTexture: imported }, identity) => {
      let frame;
      try {
        frame = imported.getVideoFrame();
        const recordStarted = performance.now();
        device.pushErrorScope('validation');
        const image = device.importExternalTexture({ source: frame });
        const binding = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [
          { binding: 0, resource: image }, { binding: 1, resource: sampler }
        ]});
        const encoder = device.createCommandEncoder();
        const pass = encoder.beginRenderPass({ colorAttachments: [{ view: context.getCurrentTexture().createView(),
          clearValue: [0, 0, 0, 1], loadOp: 'clear', storeOp: 'store' }] });
        pass.setPipeline(pipeline); pass.setBindGroup(0, binding); pass.draw(3); pass.end();
        device.queue.submit([encoder.finish()]);
        const recordCpuMs = performance.now() - recordStarted;
        // This awaits asynchronously; it never blocks the NanaUI/UI thread.
        await device.queue.onSubmittedWorkDone();
        const error = await device.popErrorScope();
        if (error) throw new Error(error.message);
        ipcRenderer.send('bridge-displayed', { ...identity, record_cpu_ms: recordCpuMs });
      } catch (error) { ipcRenderer.send('bridge-error', bridgeFailure(error)); }
      finally {
        frame?.close();
        imported.release();
      }
    });
    ipcRenderer.send('bridge-ready');
  } catch (error) { ipcRenderer.send('bridge-error', bridgeFailure(error)); }
});
