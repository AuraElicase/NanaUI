'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const { TextureLifecycle } = require('./lifecycle.cjs');
const { FrameMeasurements } = require('./measurements.cjs');

test('the live coordinators split 270 completed GPU receipts into 30 warmup and 240 measured', () => {
  const measurements = new FrameMeasurements(30, 240);
  let releases = 0;
  const lifecycle = new TextureLifecycle({
    onDisplayed: (frame, ms) => measurements.record(frame.id, ms),
    onRelease() { releases++; }
  });
  for (let id = 1; id <= 270; id++) {
    const frame = { id, pool: 2, device_generation: 3, resource_generation: 4 };
    lifecycle.add(frame);
    if (id % 2) lifecycle.allReferencesReleased(frame);
    lifecycle.displayed(frame, id <= 30 ? 10000 : 0.125);
    if (!(id % 2)) lifecycle.allReferencesReleased(frame);
  }
  assert.equal(releases, 270);
  assert.equal(lifecycle.pendingCount, 0);
  const report = measurements.report();
  assert.equal(report.consumer_work.displayed_frames, 240);
  assert.equal(report.warmup_consumer_work.displayed_frames, 30);
  assert.equal(report.total_consumer_work.displayed_frames, 270);
  assert.deepEqual(report.consumer_record_cpu_ms, { samples: 240, p50: 0.125, p95: 0.125, p99: 0.125 });
  assert.equal(report.warmup_consumer_record_cpu_ms.p50, 10000);
});

test('missing, infinite, negative and out-of-range timing cannot masquerade as measured work', () => {
  const measurements = new FrameMeasurements(30, 240);
  for (const ms of [undefined, NaN, Infinity, -1, true, '0.2'])
    assert.throws(() => measurements.record(31, ms));
  for (const id of [0, 271, NaN]) assert.throws(() => measurements.record(id, 0.2));
  assert.deepEqual(measurements.report().consumer_record_cpu_ms, { samples: 0, p50: null, p95: null, p99: null });
});

test('a missing consumer timing cannot emit display or release acknowledgement', () => {
  const measurements = new FrameMeasurements(30, 240);
  let releases = 0;
  const lifecycle = new TextureLifecycle({
    onDisplayed: (frame, ms) => measurements.record(frame.id, ms),
    onRelease() { releases++; }
  });
  const frame = { id: 1, pool: 2, device_generation: 3, resource_generation: 4 };
  lifecycle.add(frame);
  lifecycle.allReferencesReleased(frame);
  assert.throws(() => lifecycle.displayed(frame));
  assert.equal(releases, 0);
  assert.equal(measurements.totalCount, 0);
  assert.equal(lifecycle.pendingCount, 1);
  lifecycle.displayed(frame, 0.25);
  assert.equal(releases, 1);
});
