'use strict';

const unavailableCapabilities = new Set(['webgpu-api', 'webgpu-adapter', 'webgpu-canvas']);

class UnsupportedCapability extends Error {
  constructor(capability, message) {
    super(message);
    this.capability = capability;
  }
}

function bridgeFailure(error) {
  if (error instanceof UnsupportedCapability && unavailableCapabilities.has(error.capability))
    return { kind: 'unsupported-capability', capability: error.capability, message: error.message };
  return { kind: 'runtime-error', message: String(error) };
}

function classifyBridgeFailure(payload) {
  const message = typeof payload?.message === 'string' ? payload.message : 'Unexpected renderer failure';
  if (payload?.kind === 'unsupported-capability' && unavailableCapabilities.has(payload.capability))
    return { report: { status: 'unsupported', evidence: null, reason: message }, exitCode: 2 };
  return { report: { status: 'error', evidence: null, error: message }, exitCode: 1 };
}

function classifyProducerSummary(value) {
  if (value.status === 'ok') return { report: value, exitCode: 0 };
  if (value.status === 'unsupported') return { report: value, exitCode: 2 };
  return { report: { ...value, status: 'error', evidence: null,
    error: typeof value.error === 'string' ? value.error : 'Invalid producer completion status' }, exitCode: 1 };
}

module.exports = { UnsupportedCapability, bridgeFailure, classifyBridgeFailure, classifyProducerSummary };
