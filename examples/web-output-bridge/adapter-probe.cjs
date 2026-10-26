'use strict';
// Capability diagnostics only. This never imports a NanaUI texture or claims
// that a successful adapter request validates the foreign-consumer protocol.
const { app, BrowserWindow } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
app.commandLine.appendSwitch('use-angle', 'd3d11');

function finish(value, code) {
  const report = { schema_version: 1, fixture: 'electron-webgpu-capability',
    electron_version: process.versions.electron, ...value };
  const index = process.argv.indexOf('--output');
  if (index >= 0) fs.writeFileSync(path.resolve(process.argv[index + 1]), JSON.stringify(report, null, 2));
  process.stdout.write(JSON.stringify(report) + '\n');
  app.exit(code);
}

app.whenReady().then(async () => {
  const window = new BrowserWindow({ show: true, width: 680, height: 420,
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true } });
  await window.loadFile(path.join(__dirname, 'index.html'));
  const capability = await window.webContents.executeJavaScript(`(async () => {
    const result = { secure_context: isSecureContext, webgpu_api: !!navigator.gpu, cases: [] };
    if (!navigator.gpu) return result;
    for (const options of [{ powerPreference: 'high-performance' }, { forceFallbackAdapter: true }]) {
      const entry = { options, adapter: null, device_created: false };
      try {
        const adapter = await navigator.gpu.requestAdapter(options);
        if (adapter) {
          const info = adapter.info;
          entry.adapter = { vendor: info.vendor, architecture: info.architecture,
            device: info.device, description: info.description, is_fallback_adapter: info.isFallbackAdapter };
          const device = await adapter.requestDevice();
          entry.device_created = true;
          device.destroy();
        }
      } catch (error) { entry.error = String(error); }
      result.cases.push(entry);
    }
    return result;
  })()`);
  const available = capability.cases.some(entry => entry.device_created);
  const status = capability.cases.some(entry => entry.error) ? 'error' : available ? 'ok' : 'unsupported';
  finish({ status,
    evidence: 'actual-electron-webgpu-capability', capability,
    gpu_feature_status: app.getGPUFeatureStatus(),
    scope: 'Adapter/device creation only; no shared-texture import, draw, release or performance evidence' }, status === 'ok' ? 0 : status === 'unsupported' ? 2 : 1);
}).catch(error => finish({ status: 'error', evidence: null, error: String(error) }, 1));
