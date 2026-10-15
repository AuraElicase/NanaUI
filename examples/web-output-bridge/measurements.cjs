'use strict';

class FrameMeasurements {
  constructor(warmupFrames, frames) {
    if (!Number.isSafeInteger(warmupFrames) || warmupFrames < 0 || !Number.isSafeInteger(frames) || frames <= 0 || frames > 10000)
      throw new Error('Invalid frame measurement range');
    this.warmupFrames = warmupFrames;
    this.frames = frames;
    this.warmup = [];
    this.measured = [];
  }

  record(id, recordCpuMs) {
    if (!Number.isSafeInteger(id) || id <= 0 || id > this.warmupFrames + this.frames
        || typeof recordCpuMs !== 'number' || !Number.isFinite(recordCpuMs) || recordCpuMs < 0)
      throw new Error('Invalid measured frame or CPU sample');
    (id <= this.warmupFrames ? this.warmup : this.measured).push(recordCpuMs);
  }

  get measuredCount() { return this.measured.length; }
  get totalCount() { return this.warmup.length + this.measured.length; }

  report() {
    const work = count => ({ displayed_frames: count, submissions: count, draw_calls: count });
    const quantiles = samples => {
      if (samples.length === 0) return { samples: 0, p50: null, p95: null, p99: null };
      const sorted = [...samples].sort((a, b) => a - b);
      const at = p => sorted[Math.round((sorted.length - 1) * p)];
      return { samples: sorted.length, p50: at(0.5), p95: at(0.95), p99: at(0.99) };
    };
    return {
      consumer_work: work(this.measured.length),
      warmup_consumer_work: work(this.warmup.length),
      total_consumer_work: work(this.totalCount),
      consumer_record_cpu_ms: quantiles(this.measured),
      warmup_consumer_record_cpu_ms: quantiles(this.warmup)
    };
  }
}

module.exports = { FrameMeasurements };
