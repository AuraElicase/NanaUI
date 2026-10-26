'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { createRequire } = require('node:module');
const { runInNewContext } = require('node:vm');
const { UnsupportedCapability, bridgeFailure, classifyBridgeFailure, classifyProducerSummary } = require('./failures.cjs');

test('only explicit missing-capability conditions become platform gaps', () => {
  for (const capability of ['webgpu-api', 'webgpu-adapter', 'webgpu-canvas']) {
    const result = classifyBridgeFailure(bridgeFailure(new UnsupportedCapability(capability, 'unavailable')));
    assert.equal(result.report.status, 'unsupported');
    assert.equal(result.exitCode, 2);
    assert.equal(result.report.reason, 'unavailable');
  }
});

test('shader validation, lost device, queue failure and unexpected exceptions are errors', () => {
  for (const message of ['WGSL validation failed', 'WebGPU device lost', 'queue submission failed', 'unexpected exception']) {
    const result = classifyBridgeFailure(bridgeFailure(new Error(message)));
    assert.equal(result.report.status, 'error');
    assert.equal(result.exitCode, 1);
    assert.match(result.report.error, new RegExp(message));
  }
});

test('renderer supplied status values and unknown tags cannot hide runtime errors', () => {
  for (const payload of [
    { status: 'unsupported', message: 'validation failure' },
    { kind: 'runtime-error', status: 'unsupported', message: 'lost device' },
    { kind: 'unsupported-capability', capability: 'shader-validation', message: 'broken shader' },
    { kind: 'unsupported', message: 'unknown tag' },
    'legacy untyped error', null
  ]) {
    const result = classifyBridgeFailure(payload);
    assert.equal(result.report.status, 'error');
    assert.equal(result.exitCode, 1);
  }
});

test('a forged or unknown capability exception is not classified as unavailable', () => {
  for (const error of [new UnsupportedCapability('device-lost', 'lost'), { kind: 'unsupported-capability', capability: 'webgpu-api' }]) {
    assert.equal(classifyBridgeFailure(bridgeFailure(error)).exitCode, 1);
  }
});

test('producer completion preserves error versus unsupported exit status', () => {
  for (const [status, expectedExit] of [['ok', 0], ['unsupported', 2], ['error', 1], ['unknown', 1], [undefined, 1]]) {
    const result = classifyProducerSummary({ type: 'summary', status });
    assert.equal(result.exitCode, expectedExit);
    assert.equal(result.report.status, status === 'ok' || status === 'unsupported' ? status : 'error');
  }
});

test('the real host accepts the package-pinned Electron and rejects unsupported hosts before opening a window', async () => {
  const mainPath = require.resolve('./main.cjs');
  const mainRequire = createRequire(mainPath);
  const pinnedVersion = mainRequire('./package.json').devDependencies.electron;
  const source = readFileSync(mainPath, 'utf8');
  for (const [platform, arch, version, hasApi, accepted] of [
    ['win32', 'x64', pinnedVersion, true, true],
    ['win32', 'x64', '44.7.2', true, false],
    ['darwin', 'x64', pinnedVersion, true, false],
    ['win32', 'arm64', pinnedVersion, true, false],
    ['win32', 'x64', pinnedVersion, false, false]
  ]) {
    let opened = 0;
    let exitCode;
    let report;
    const electron = {
      app: {
        commandLine: { appendSwitch() {} },
        whenReady: () => Promise.resolve(),
        exit: code => { exitCode = code; }
      },
      ipcMain: { on() {} },
      sharedTexture: hasApi ? { importSharedTexture() {} } : undefined,
      BrowserWindow: class {
        constructor() {
          opened++;
          this.webContents = { setWindowOpenHandler() {}, on() {} };
        }
        on() {}
        loadFile() { return Promise.resolve(); }
      }
    };
    await runInNewContext(source, {
      require: name => name === 'electron' ? electron : mainRequire(name),
      __dirname,
      process: {
        platform, arch, versions: { electron: version }, argv: [],
        stdout: { write: value => { report = JSON.parse(value); } }
      }
    }, { filename: mainPath });
    assert.equal(opened, accepted ? 1 : 0);
    assert.equal(exitCode, accepted ? undefined : 2);
    if (!accepted) {
      assert.equal(report.status, 'unsupported');
      assert.equal(report.evidence, null);
      assert.ok(report.reason.includes(pinnedVersion));
    }
  }
});
