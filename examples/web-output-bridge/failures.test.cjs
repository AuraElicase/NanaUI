'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
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
