'use strict';

const identityKeys = ['id', 'pool', 'device_generation', 'resource_generation'];
function validateIdentity(frame) {
  if (!frame || identityKeys.some(key => !Number.isSafeInteger(frame[key]) || frame[key] <= 0))
    throw new Error('Invalid frame identity');
}

// The actual main process uses this coordinator. Only the trusted Electron
// callback reaches allReferencesReleased; renderer IPC can reach displayed.
class TextureLifecycle {
  constructor({ onDisplayed, onRelease }) {
    this.onDisplayed = onDisplayed;
    this.onRelease = onRelease;
    this.frames = new Map();
    this.pending = 0;
    this.lastFrameId = 0;
    this.stopped = false;
  }

  add(frame) {
    validateIdentity(frame);
    if (this.stopped) throw new Error('Texture transport is disconnected');
    if (this.frames.has(frame.id)) throw new Error('Duplicate imported frame');
    if (frame.id <= this.lastFrameId) throw new Error('Stale imported frame');
    if (this.pending !== 0) throw new Error('Native transport already has a pending frame');
    if (this.frames.size >= 10030) throw new Error('Reference fixture frame budget exceeded');
    this.frames.set(frame.id, { frame: Object.freeze({ ...frame }), displayed: false, referencesReleased: false, released: false });
    this.lastFrameId = frame.id;
    this.pending++;
  }

  lookup(identity) {
    validateIdentity(identity);
    if (this.stopped) throw new Error('Texture transport is disconnected');
    const state = this.frames.get(identity.id);
    if (!state || identityKeys.some(key => identity[key] !== state.frame[key]))
      throw new Error('Stale or wrong-generation frame receipt');
    return state;
  }

  displayed(identity, recordCpuMs) {
    const state = this.lookup(identity);
    if (typeof recordCpuMs !== 'number' || !Number.isFinite(recordCpuMs) || recordCpuMs < 0)
      throw new Error('Missing or invalid consumer CPU timing');
    if (state.displayed) throw new Error('Duplicate GPU display receipt');
    state.displayed = true;
    this.onDisplayed(state.frame, recordCpuMs);
    this.releaseIfComplete(state);
  }

  allReferencesReleased(identity) {
    const state = this.lookup(identity);
    if (state.referencesReleased) throw new Error('Duplicate Electron release callback');
    state.referencesReleased = true;
    this.releaseIfComplete(state);
  }

  releaseIfComplete(state) {
    if (state.released || !state.displayed || !state.referencesReleased || this.stopped) return;
    state.released = true;
    this.pending--;
    this.onRelease(state.frame);
  }

  failSend(identity) {
    this.lookup(identity);
    this.disconnect();
  }

  disconnect() {
    // No synthetic allReferencesReleased or release callback on teardown.
    this.stopped = true;
  }

  get pendingCount() { return this.pending; }
}

module.exports = { TextureLifecycle };
