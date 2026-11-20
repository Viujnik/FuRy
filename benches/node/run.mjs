#!/usr/bin/env node
import { writeFile, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { runBench } from './harness.mjs';
import { prepareVariant } from './variants.mjs';

const { values } = parseArgs({ options: {
  variant: { type: 'string' }, out: { type: 'string' }, fixtures: { type: 'string' },
  warmup: { type: 'string', default: '24' }, iterations: { type: 'string', default: '50' },
} });
if (!values.variant || !values.out) throw new Error('Pass --variant and --out');
const fixtureDir = resolve(values.fixtures ?? fileURLToPath(new URL('../fixtures', import.meta.url)));
const warmup = Number(values.warmup);
const iterations = Number(values.iterations);
if (![warmup, iterations].every((n) => Number.isSafeInteger(n) && n <= 10000)
    || warmup < 0 || iterations < 1) throw new Error('Invalid warmup or iteration count');
const { run, validate, metadata } = await prepareVariant(values.variant, fixtureDir);
const samples = runBench(values.variant, run, validate, { warmup, iterations });
await mkdir(dirname(values.out), { recursive: true });
await writeFile(values.out, JSON.stringify({ ...metadata, ...samples }, null, 2));
console.log(`  ${values.variant}: p50=${samples.median_ms.toFixed(4)} ms p95=${samples.p95_ms.toFixed(4)} ms`);
