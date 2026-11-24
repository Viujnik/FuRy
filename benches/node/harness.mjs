import { createHash } from 'node:crypto';

export function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export function splitFuryFrames(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const frames = [];
  for (let start = 0; start < bytes.length;) {
    if (bytes.length - start < 6) throw new Error('Truncated Fury header');
    const length = view.getUint32(start, true);
    if (length < 6 || length > bytes.length - start) throw new Error('Invalid Fury frame length');
    if (bytes[start + 4] !== 70 || ![2, 3].includes(bytes[start + 5])) throw new Error('Incorrect Fury version');
    frames.push(bytes.subarray(start, start + length));
    start += length;
  }
  return frames;
}

export function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const i = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[i] : (sorted[i - 1] + sorted[i]) / 2;
}

export function runBench(variant, run, validate, { warmup = 24, iterations = 50 }) {
  if (typeof globalThis.gc !== 'function') throw new Error('Run with --expose-gc');
  for (let i = 0; i < warmup; i++) validate(run());
  const samples = [];
  for (let i = 0; i < iterations; i++) {
    globalThis.gc();
    const start = process.hrtime.bigint();
    const result = run();
    samples.push(Number(process.hrtime.bigint() - start) / 1e6);
    validate(result);
  }
  const sorted = [...samples].sort((a, b) => a - b);
  const medianMs = median(samples);
  return {
    variant, warmup, iterations, median_ms: medianMs,
    p95_ms: sorted[Math.ceil(sorted.length * 0.95) - 1],
    min_ms: sorted[0], max_ms: sorted.at(-1), samples_ms: samples,
    rss_bytes: process.memoryUsage().rss,
    rss_kind: 'whole-process end-of-run sample',
  };
}
