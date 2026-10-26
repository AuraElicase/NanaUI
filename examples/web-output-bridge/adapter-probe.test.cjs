'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, 'adapter-probe.cjs'), 'utf8');

async function probe(gpu) {
  let code;
  let report;
  const electron = {
    app: { commandLine: { appendSwitch() {} }, whenReady: () => Promise.resolve(),
      getGPUFeatureStatus: () => ({ webgpu: 'test' }), exit(value) { code = value; } },
    BrowserWindow: class {
      webContents = { executeJavaScript: script => vm.runInNewContext(script,
        { navigator: { gpu }, isSecureContext: true }) };
      async loadFile() {}
    }
  };
  await vm.runInNewContext(source, {
    require: name => name === 'electron' ? electron : require(name),
    __dirname, process: { argv: [], versions: { electron: 'test' },
      stdout: { write: text => { report = JSON.parse(text); } } }
  });
  return { code, report };
}

for (const [name, gpu] of [['missing WebGPU API', undefined],
  ['no adapter in either mode', { requestAdapter: async () => null }]]) {
  test(name + ' remains unsupported with no consumer evidence', async () => {
    const { code, report } = await probe(gpu);
    assert.equal(code, 2);
    assert.equal(report.status, 'unsupported');
    assert.equal(report.fixture, 'electron-webgpu-capability');
    assert.equal(report.frames, undefined);
  });
}

test('an explicit fallback device is reported separately from the default request', async () => {
  let destroyed = 0;
  const { code, report } = await probe({ requestAdapter: async options => options.forceFallbackAdapter ? {
    info: { vendor: 'test', architecture: 'software', device: 'test', description: 'test', isFallbackAdapter: true },
    requestDevice: async () => ({ destroy() { destroyed++; } })
  } : null });
  assert.equal(code, 0);
  assert.equal(report.capability.cases[0].adapter, null);
  assert.equal(report.capability.cases[1].adapter.is_fallback_adapter, true);
  assert.equal(report.capability.cases[1].device_created, true);
  assert.equal(destroyed, 1);
  assert.equal(report.evidence, 'actual-electron-webgpu-capability');
});

test('device creation errors cannot be relabeled as missing adapters', async () => {
  const { code, report } = await probe({ requestAdapter: async () => ({ info: {},
    requestDevice: async () => { throw new Error('device lost'); } }) });
  assert.equal(code, 1);
  assert.equal(report.status, 'error');
  assert.match(report.capability.cases[0].error, /device lost/);
});
