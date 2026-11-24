import { median } from './harness.mjs';

export function comparePairs(base, candidate, comparisons) {
  if (!base.length || base.length !== candidate.length || !Number.isInteger(comparisons)
      || comparisons < 1 || [...base, ...candidate].some((x) => !Number.isFinite(x) || x <= 0)) {
    throw new Error('Invalid paired latency samples');
  }
  const logBase = base.map(Math.log);
  const center = median(logBase);
  const noise = median(logBase.map((x) => Math.abs(x - center)));
  const deltas = candidate.map((x, i) => Math.log(x) - logBase[i]);
  const count = deltas.filter((d) => d > noise).length;
  // One-sided paired sign test. Values within the baseline MAD count against a slowdown.
  let coefficient = 1;
  let upperTail = 0;
  for (let k = 0; k <= deltas.length; k++) {
    if (k >= count) upperTail += coefficient;
    coefficient *= (deltas.length - k) / (k + 1);
  }
  const p = upperTail / 2 ** deltas.length;
  const alpha = 0.05 / comparisons;
  return {
    base_median_ms: median(base), candidate_median_ms: median(candidate),
    paired_median_change_pct: Math.expm1(median(deltas)) * 100,
    baseline_noise_pct: Math.expm1(noise) * 100, pairs: base.length,
    pairs_over_noise: count, one_sided_p: p, alpha, flagged: p < alpha,
  };
}
