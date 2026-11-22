import assert from 'node:assert/strict';
import test from 'node:test';
import { median, splitFuryFrames } from './harness.mjs';
import { comparePairs } from './compare.mjs';

const frame = () => Uint8Array.of(6, 0, 0, 0, 70, 2);

test('split consumes complete ordered frames at a nonzero source offset', () => {
  const bytes = Uint8Array.from([0, ...frame(), ...frame(), 0]).subarray(1, 13);
  const frames = splitFuryFrames(bytes);
  assert.deepEqual(frames, [frame(), frame()]);
  assert.equal(frames[1].byteOffset, bytes.byteOffset + 6);
  assert.equal(frames[1].buffer, bytes.buffer);
});

test('split rejects truncated frames and incorrect headers', () => {
  const full = frame();
  for (let i = 1; i < full.length; i++) assert.throws(() => splitFuryFrames(full.subarray(0, i)));
  for (const index of [0, 4, 5]) {
    const changed = frame();
    changed[index] = 0;
    assert.throws(() => splitFuryFrames(changed));
  }
});

test('paired comparison flags a consistent slowdown beyond observed noise', () => {
  const baseline = [1, 1.002, 1.003, 1.004, 1.005, 1.006, 1.007, 1.008];
  const slower = baseline.map((value) => value * 1.1);
  const report = comparePairs(baseline, slower, 3);
  assert.equal(report.pairs_over_noise, 8);
  assert.equal(report.one_sided_p, 1 / 256);
  assert.ok(report.flagged && Math.abs(report.paired_median_change_pct - 10) < 1e-8);
});

test('observed jitter and mixed signs stay below the regression gate', () => {
  const base = [1, 2, 1, 2, 1, 2, 1, 2];
  const tiny = base.map((value) => value * 1.01);
  assert.equal(comparePairs(base, tiny, 3).flagged, false);
  assert.equal(comparePairs(base, [...base].reverse(), 3).flagged, false);
});

test('reject mismatched sample sizes and non-finite metrics', () => {
  assert.throws(() => comparePairs([1, 1], [1], 3));
  assert.throws(() => comparePairs([0], [1], 3));
  assert.throws(() => comparePairs([1], [NaN], 3));
  assert.equal(median([4, 1, 2, 3]), 2.5);
});
