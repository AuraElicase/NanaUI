'use strict';
const { app, BrowserWindow, ipcMain, sharedTexture } = require('electron');
const { spawn } = require('node:child_process');
const { createInterface } = require('node:readline');
const path = require('node:path');
const fs = require('node:fs');
const { TextureLifecycle } = require('./lifecycle.cjs');
const { FrameMeasurements } = require('./measurements.cjs');
const { classifyBridgeFailure, classifyProducerSummary } = require('./failures.cjs');
const electronVersion = require('./package.json').devDependencies.electron;

// This is a foreign host fixture, never the NanaUI product shell.
app.commandLine.appendSwitch('use-angle', 'd3d11');
let producer;
let window;
let finished = false;
let report;
const argument = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : fallback;
};
const measurements = new FrameMeasurements(30, Number(argument('--frames', '240')));
const live = new Map();
const lifecycle = new TextureLifecycle({
  onDisplayed(frame, recordCpuMs) {
    measurements.record(frame.id, recordCpuMs);
    send(frame, 'displayed');
  },
  onRelease(frame) {
    live.delete(frame.id);
    send(frame, 'released');
  }
});
function finish(value, exitCode) {
  if (finished) return;
  finished = true;
  lifecycle.disconnect();
  report = { schema_version: 1, fixture: 'electron-webgpu-output', fixture_version: 1, platform: process.platform === 'win32' ? 'windows' : process.platform, electron_version: process.versions.electron,
    ...value, ...measurements.report(),
    chromium_internal_gpu_copies: null, gpu_time_ms: null, driver_vram_bytes: null };
  const output = argument('--output', null);
  if (output) fs.writeFileSync(path.resolve(output), JSON.stringify(report, null, 2));
  process.stdout.write(JSON.stringify(report) + '\n');
  // EOF quarantines unconfirmed native tokens; it never simulates a release.
  producer?.stdin.end();
  app.exit(exitCode);
}

function send(frame, type) {
  if (!producer?.stdin.writable) return;
  producer.stdin.write(JSON.stringify({ type, id: frame.id, pool: frame.pool,
    device_generation: frame.device_generation, resource_generation: frame.resource_generation }) + '\n');
}

function receiveFrame(frame) {
  if (frame.alpha !== 'opaque' || frame.color_space !== 'srgb' || frame.pixel_format !== 'bgra' || !frame.ready_observed)
    throw new Error('Unsupported texture encoding or missing producer ready proof');
  lifecycle.add(frame);
  const handle = Buffer.alloc(8);
  handle.writeBigUInt64LE(BigInt('0x' + frame.nt_handle));
  let imported;
  // The managed API duplicates this NT HANDLE. Rust closes its temporary
  // duplicate only after the successful synchronous import acknowledgement.
  imported = sharedTexture.importSharedTexture({
    textureInfo: {
      pixelFormat: 'bgra', codedSize: { width: frame.width, height: frame.height },
      visibleRect: { x: 0, y: 0, width: frame.width, height: frame.height },
      colorSpace: { primaries: 'bt709', transfer: 'srgb', matrix: 'rgb', range: 'full' },
      timestamp: frame.id * 16667, handle: { ntHandle: handle }
    },
    allReferencesReleased() {
      // Only this main-process Electron callback authorizes native reuse.
      // Renderer IPC is deliberately unable to generate a release receipt.
      if (finished) return;
      try { lifecycle.allReferencesReleased(frame); }
      catch (error) { finish({ status: 'error', evidence: null, error: String(error), has_pending_gpu_reads: true }, 1); }
    }
  });
  live.set(frame.id, { frame, imported });
  send(frame, 'imported');
  sharedTexture.sendSharedTexture({ frame: window.webContents.mainFrame, importedSharedTexture: imported },
    { id: frame.id, pool: frame.pool, device_generation: frame.device_generation, resource_generation: frame.resource_generation })
    .catch(error => {
      if (finished) return;
      lifecycle.failSend(frame);
      finish({ status: 'error', evidence: null, error: String(error), has_pending_gpu_reads: true }, 1);
    })
    .finally(() => imported.release());
}

ipcMain.on('bridge-ready', event => {
  if (event.sender !== window.webContents || producer) return;
  const executable = path.resolve(argument('--producer', path.join(__dirname, '../../target/release/examples/web-output-producer.exe')));
  producer = spawn(executable, ['--electron-pid', String(process.pid), '--frames', argument('--frames', '240')],
    { stdio: ['pipe', 'pipe', 'inherit'], windowsHide: true });
  producer.on('error', error => finish({ status: 'error', evidence: null, error: String(error) }, 1));
  // 'exit' may precede stdout drainage. 'close' follows stdio closure, so
  // a flushed final summary is parsed before we reject a missing summary.
  producer.on('close', code => {
    if (!finished) finish({ status: 'error', evidence: null, error: `Producer exited ${code} without a successful summary`, has_pending_gpu_reads: lifecycle.pendingCount !== 0 }, 1);
  });
  createInterface({ input: producer.stdout }).on('line', line => {
    try {
      const value = JSON.parse(line);
      if (value.type === 'texture') receiveFrame(value);
      else if (value.type === 'summary') {
        if (value.status === 'ok' && (lifecycle.pendingCount !== 0 || value.proof?.displayed_frames !== measurements.measuredCount || value.total_frames !== measurements.totalCount || measurements.measuredCount !== measurements.frames || measurements.totalCount !== measurements.warmupFrames + measurements.frames))
          throw new Error('Completion report disagrees with imported GPU frames');
        const result = classifyProducerSummary(value);
        finish(result.report, result.exitCode);
      } else throw new Error('Unknown producer message');
    } catch (error) { finish({ status: 'error', evidence: null, error: String(error), has_pending_gpu_reads: lifecycle.pendingCount !== 0 }, 1); }
  });
});

ipcMain.on('bridge-displayed', (event, identity) => {
  if (finished || event.sender !== window.webContents) return;
  try { lifecycle.displayed(identity, identity?.record_cpu_ms); }
  catch (error) { finish({ status: 'error', evidence: null, error: String(error), has_pending_gpu_reads: true }, 1); }
});
ipcMain.on('bridge-error', (event, error) => {
  if (event.sender === window.webContents) {
    const result = classifyBridgeFailure(error);
    finish({ ...result.report, has_pending_gpu_reads: lifecycle.pendingCount !== 0 }, result.exitCode);
  }
});
app.whenReady().then(() => {
  if (process.platform !== 'win32' || process.arch !== 'x64' || process.versions.electron !== electronVersion || typeof sharedTexture?.importSharedTexture !== 'function')
    return finish({ status: 'unsupported', evidence: null, reason: `Windows x64 Electron ${electronVersion} managed sharedTexture API is required` }, 2);
  window = new BrowserWindow({ width: 680, height: 420, webPreferences: {
    preload: path.join(__dirname, 'preload.cjs'), sandbox: false, contextIsolation: true, nodeIntegration: false
  }});
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', event => event.preventDefault());
  window.webContents.on('render-process-gone', () => finish({ status: 'error', evidence: null, error: 'Renderer disconnected', has_pending_gpu_reads: lifecycle.pendingCount !== 0 }, 1));
  window.on('closed', () => { if (!finished) finish({ status: 'error', evidence: null, error: 'Window closed before GPU retirement', has_pending_gpu_reads: lifecycle.pendingCount !== 0 }, 1); });
  return window.loadFile(path.join(__dirname, 'index.html'));
}).catch(error => finish({ status: 'error', evidence: null, error: String(error) }, 1));
