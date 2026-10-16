'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { TextureLifecycle } = require('./lifecycle.cjs');

const frame = (id = 1) => ({ id, pool: 10, device_generation: 20, resource_generation: 30 });
function fixture() {
  const events = [];
  const lifecycle = new TextureLifecycle({
    onDisplayed: value => events.push(['displayed', value.id]),
    onRelease: value => events.push(['released', value.id])
  });
  lifecycle.add(frame());
  return { lifecycle, events };
}

for (const first of ['electron', 'renderer']) {
  test(`one release after both proofs: ${first} first`, () => {
    const { lifecycle, events } = fixture();
    if (first === 'electron') lifecycle.allReferencesReleased(frame());
    else lifecycle.displayed(frame(), 0.25);
    assert.equal(lifecycle.pendingCount, 1);
    assert.equal(events.filter(([kind]) => kind === 'released').length, 0);
    if (first === 'electron') lifecycle.displayed(frame(), 0.25);
    else lifecycle.allReferencesReleased(frame());
    assert.deepEqual(events, [['displayed', 1], ['released', 1]]);
    assert.equal(lifecycle.pendingCount, 0);
    assert.throws(() => lifecycle.displayed(frame(), 0.25), /Duplicate/);
    assert.throws(() => lifecycle.allReferencesReleased(frame()), /Duplicate/);
    assert.equal(events.filter(([kind]) => kind === 'released').length, 1);
  });
}

test('renderer acknowledgement alone never authorizes native reuse', () => {
  const { lifecycle, events } = fixture();
  lifecycle.displayed(frame(), 0.25);
  assert.deepEqual(events, [['displayed', 1]]);
  assert.equal(lifecycle.pendingCount, 1);
  assert.throws(() => lifecycle.displayed(frame(), 0.25), /Duplicate/);
  assert.equal(events.filter(([kind]) => kind === 'released').length, 0);
});

test('stale, invalid and wrong-generation receipts cannot advance state', () => {
  const { lifecycle, events } = fixture();
  for (const invalid of [frame(2), { ...frame(), id: 0 }, { ...frame(), id: NaN },
    { ...frame(), pool: 11 }, { ...frame(), device_generation: 21 }, { ...frame(), resource_generation: 31 }]) {
    assert.throws(() => lifecycle.displayed(invalid, 0.25));
    assert.throws(() => lifecycle.allReferencesReleased(invalid));
  }
  assert.deepEqual(events, []);
  assert.equal(lifecycle.pendingCount, 1);
  lifecycle.displayed(frame(), 0.25);
  lifecycle.allReferencesReleased(frame());
  assert.equal(lifecycle.pendingCount, 0);
});

test('an imported duplicate or a second in-flight frame is rejected', () => {
  const { lifecycle, events } = fixture();
  assert.throws(() => lifecycle.add(frame()), /Duplicate/);
  assert.throws(() => lifecycle.add(frame(2)), /pending/);
  assert.deepEqual(events, []);
  lifecycle.displayed(frame(), 0.25);
  lifecycle.allReferencesReleased(frame());
  lifecycle.add(frame(2));
  assert.equal(lifecycle.pendingCount, 1);
});

test('send failure preserves unresolved ownership and cannot forge release', () => {
  const { lifecycle, events } = fixture();
  lifecycle.failSend(frame());
  assert.equal(lifecycle.pendingCount, 1);
  assert.throws(() => lifecycle.displayed(frame(), 0.25), /disconnected/);
  assert.throws(() => lifecycle.allReferencesReleased(frame()), /disconnected/);
  assert.throws(() => lifecycle.add(frame(2)), /disconnected/);
  assert.deepEqual(events, []);
});

test('late imported frames cannot move the published sequence backwards', () => {
  const { lifecycle } = fixture();
  lifecycle.displayed(frame(), 0.25);
  lifecycle.allReferencesReleased(frame());
  lifecycle.add(frame(3));
  lifecycle.displayed(frame(3), 0.25);
  lifecycle.allReferencesReleased(frame(3));
  assert.throws(() => lifecycle.add(frame(2)), /Stale/);
  assert.equal(lifecycle.pendingCount, 0);
});

for (const proof of ['none', 'electron', 'renderer']) {
  test(`disconnect after ${proof} proof never fabricates retirement`, () => {
    const { lifecycle, events } = fixture();
    if (proof === 'electron') lifecycle.allReferencesReleased(frame());
    if (proof === 'renderer') lifecycle.displayed(frame(), 0.25);
    lifecycle.disconnect();
    assert.equal(lifecycle.pendingCount, 1);
    assert.throws(() => lifecycle.allReferencesReleased(frame()), /disconnected/);
    assert.equal(events.filter(([kind]) => kind === 'released').length, 0);
  });
}

test('identity snapshots cannot be rewritten by a caller after import', () => {
  const events = [];
  const lifecycle = new TextureLifecycle({ onDisplayed() {}, onRelease: value => events.push(value.pool) });
  const original = frame();
  lifecycle.add(original);
  original.pool = 99;
  assert.throws(() => lifecycle.displayed(original, 0.25), /wrong-generation/);
  lifecycle.displayed(frame(), 0.25);
  lifecycle.allReferencesReleased(frame());
  assert.deepEqual(events, [10]);
});
