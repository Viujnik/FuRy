#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { arch, cpus, platform, release, tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { encode } from '@msgpack/msgpack';
import { sha256 } from './harness.mjs';
import { ALL_VARIANTS, loadWasm } from './variants.mjs';
import { comparePairs } from './compare.mjs';

const { values } = parseArgs({ options: {
  smoke: { type: 'boolean' }, out: { type: 'string' }, 'baseline-wasm': { type: 'string' },
  fixtures: { type: 'string' }, rows: { type: 'string' }, trials: { type: 'string' },
  warmup: { type: 'string' }, iterations: { type: 'string' },
} });
if (!values.smoke && !values.out) throw new Error('Use --smoke or --out DIRECTORY');
if (values.smoke && values['baseline-wasm']) throw new Error('Paired measurements require a full run');
if (values.fixtures && values.rows) throw new Error('--rows is for generated fixtures');
const rows = Number(values.rows ?? (values.smoke ? 7 : 128));
const trials = Number(values.trials ?? (values.smoke ? 1 : 8));
const warmup = Number(values.warmup ?? (values.smoke ? 1 : 24));
const iterations = Number(values.iterations ?? (values.smoke ? 2 : 50));
for (const [n, min, max] of [[rows, 1, 10000], [trials, values['baseline-wasm'] ? 8 : 1, 30],
  [warmup, 0, 10000], [iterations, 1, 10000]]) {
  if (!Number.isSafeInteger(n) || n < min || n > max) throw new Error('Invalid sample configuration');
}

async function writeFixtures(dir, wasmPath) {
  const schema = fileURLToPath(new URL('./sample_schema.py', import.meta.url));
  execFileSync(process.env.FURY_BENCH_PYTHON ?? 'python3', ['-I', schema], { cwd: dir });
  const { loadRegistry, WasmRecord } = await loadWasm(wasmPath);
  const registry = await readFile(join(dir, '.fury_registry.bin'));
  loadRegistry(registry);
  const page = Array.from({ length: rows }, (_, i) => ({
    id: i + 1, username: `user_${i + 1}_🚲`, is_active: i % 3 !== 0, bio: 'a page note '.repeat(32 + i % 16),
  }));
  const frames = page.map((row) => {
    const record = new WasmRecord('BenchUser');
    try {
      record.setNumber('id', row.id);
      record.setString('username', row.username);
      record.setBool('is_active', row.is_active);
      record.setString('bio', row.bio);
      record.finish();
      return record.toBytes();
    } finally {
      record.free();
    }
  });
  const files = {
    'wide.json': Buffer.from(JSON.stringify(page)), 'wide.msgpack': encode(page),
    'wide.fury': Buffer.concat(frames), '.fury_registry.bin': registry,
  };
  const hashes = {};
  for (const [name, bytes] of Object.entries(files)) {
    hashes[name] = sha256(bytes);
    if (name !== '.fury_registry.bin') await writeFile(join(dir, name), bytes);
  }
  await writeFile(join(dir, 'metadata.json'), JSON.stringify({
    fixture_version: 2, schema_name: 'BenchUser', order_by: 'id ASC', rows,
    fury_layout: `concatenated-v${frames[0][5]}-frames`, fields: Object.keys(page[0]), sha256: hashes,
    read3_checksum: page.reduce((sum, row) => sum + row.id + row.username.length + Number(row.is_active), 0),
  }, null, 2));
}

const scratch = await mkdtemp(join(tmpdir(), 'fury-frontend-'));
try {
  const fixtures = values.fixtures ? resolve(values.fixtures) : join(scratch, 'fixtures');
  const out = values.out ? resolve(values.out) : join(scratch, 'results');
  await mkdir(out, { recursive: true });
  if ((await readdir(out)).length) throw new Error('Choose an empty result directory');
  if (!values.fixtures) {
    await mkdir(fixtures);
    await writeFixtures(fixtures);
  }
  const fixtureMetadata = JSON.parse(await readFile(join(fixtures, 'metadata.json'), 'utf8'));
  let baselineFixtures = fixtures;
  if (values['baseline-wasm'] && !values.fixtures) {
    baselineFixtures = join(scratch, 'baseline-fixtures');
    await mkdir(baselineFixtures);
    await writeFixtures(baselineFixtures, values['baseline-wasm']);
    const base = JSON.parse(await readFile(join(baselineFixtures, 'metadata.json'), 'utf8'));
    for (const file of ['wide.json', 'wide.msgpack', '.fury_registry.bin']) {
      if (base.sha256[file] !== fixtureMetadata.sha256[file]) throw new Error('Different baseline corpus');
    }
  }
  await writeFile(join(out, 'run.json'), JSON.stringify({
    node: process.versions, platform: platform(), release: release(), arch: arch(),
    cpu: cpus()[0]?.model, trials, warmup, iterations, fixture: fixtureMetadata,
    scope: 'latency of each full batch; file I/O, corpus verification and forced GC excluded',
  }, null, 2));
  const runFile = fileURLToPath(new URL('./run.mjs', import.meta.url));
  const medians = new Map();
  const paired = Boolean(values['baseline-wasm']);
  for (let trial = 0; trial < trials; trial++) {
    for (let j = 0; j < ALL_VARIANTS.length; j++) {
      const variant = ALL_VARIANTS[(j + trial) % ALL_VARIANTS.length];
      const roles = paired && variant.startsWith('fury_')
        ? (trial % 2 ? ['candidate', 'baseline'] : ['baseline', 'candidate']) : ['candidate'];
      for (const role of roles) {
        const resultPath = join(out, `${role}-${trial + 1}-${variant}.json`);
        const env = { ...process.env };
        if (role === 'baseline') env.FURY_BENCH_WASM = resolve(values['baseline-wasm']);
        execFileSync(process.execPath, ['--expose-gc', runFile, '--variant', variant, '--out', resultPath,
          '--fixtures', role === 'baseline' ? baselineFixtures : fixtures, '--warmup', String(warmup), '--iterations', String(iterations)],
        { env, stdio: 'inherit' });
        const result = JSON.parse(await readFile(resultPath, 'utf8'));
        if (!medians.has(variant)) medians.set(variant, { baseline: [], candidate: [] });
        medians.get(variant)[role].push(result.median_ms);
      }
    }
  }
  if (paired) {
    const pairs = [...medians].filter(([, values]) => values.baseline.length);
    const report = Object.fromEntries(pairs.map(([variant, values]) =>
      [variant, comparePairs(values.baseline, values.candidate, pairs.length)]));
    await writeFile(join(out, 'comparison.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
    if (Object.values(report).some(({ flagged }) => flagged)) process.exitCode = 1;
  }
} finally {
  await rm(scratch, { recursive: true, force: true });
}
