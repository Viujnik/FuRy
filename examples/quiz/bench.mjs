import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { pathToFileURL } from "node:url";
import { gzipSync } from "node:zlib";
import { Session } from "node:inspector/promises";

const [origin, site, samples, duration] = process.argv.slice(2);
const sustained = samples === "load";
const count = Number(samples);
const seconds = Number(duration);
if (sustained) {
    assert(Number.isSafeInteger(seconds) && seconds >= 1 && seconds <= 30, "Choose 1–30 seconds per case");
    const url = new URL(origin);
    assert(url.protocol === "http:" && url.hostname === "127.0.0.1", "Run load mode against the local quiz");
} else {
    assert(Number.isSafeInteger(count) && count >= 3 && count <= 1000, "Choose 3–1000 samples");
}
const wasm = await import(pathToFileURL(resolve(site, "wasm/fury_ts.js")));
await wasm.default({module_or_path: await readFile(resolve(site, "wasm/fury_ts_bg.wasm"))});
wasm.loadRegistry(new Uint8Array(await readFile(resolve(site, "registry.bin"))));

const { readLeaderboard, readDirectLeaderboard, readModelRows } =
    await import(pathToFileURL(resolve(site, "wire.js")));
const pageSize = Number(process.env.QUIZ_PAGE_SIZE ?? 128);
const decoder = new TextDecoder();
const readJson = bytes => JSON.parse(decoder.decode(bytes));
const orders = process.env.QUIZ_DATASET === "orders";
const decode = {
    json: readJson,
    "orm.json": readJson,
    "json-direct": readJson,
    "fury-models": (bytes) => readLeaderboard(bytes, pageSize),
    fury: (bytes) => readDirectLeaderboard(bytes, pageSize),
};
if (orders) {
    const { OrderEntry } = await import(pathToFileURL(resolve(site, "models/orderentry.js")));
    const fields = Object.getOwnPropertyNames(OrderEntry.prototype)
        .filter(name => name !== "$record" && Object.getOwnPropertyDescriptor(OrderEntry.prototype, name).get);
    const project = entry => {
        const row = {};
        for (const name of fields) row[name] = entry[name];
        return row;
    };
    decode["fury-models"] = bytes => readModelRows(bytes, pageSize, OrderEntry, project);
    decode.fury = bytes => wasm.decodePage("OrderEntry", bytes, pageSize);
}
if (process.env.QUIZ_COMPARE_CODECS === "1") {
    const { default: extraCodecs } = await import(pathToFileURL(process.env.QUIZ_CODEC_DECODER));
    Object.assign(decode, extraCodecs);
}
const formats = Object.keys(decode);
const cursors = orders
    ? [1, 1, 2, 1, 3, 4, 2, 5, 1, 8, 13, 2, 21, 34, 55, 64]
    : ["mixed", "cities"].includes(process.env.QUIZ_DATASET)
    ? [100000, 90000, 80000, 70000, 60000, 50000, 40000, 30000, 20000, 10000]
    : [100000];

async function measure(format, cursor) {
    const start = performance.now();
    const path = orders ? "/api/orders." : "/api/leaderboard.";
    const response = await fetch(origin + path + format + "?cursor=" + cursor, {
        signal: AbortSignal.timeout(5000),
    });
    const body = new Uint8Array(await response.arrayBuffer());
    assert.equal(response.status, 200);
    const rows = decode[format](body);
    return { ms: performance.now() - start, bytes: body.length, rows, body };
}

const references = new Map();
for (const cursor of new Set(cursors)) {
    const reference = await measure("json", cursor);
    assert.equal(reference.rows.length, pageSize);
    if (orders) assert.equal(Object.keys(reference.rows[0]).length, 11);
    references.set(cursor, reference.rows);
    let modelBytes;
    for (const format of formats.slice(1)) {
        const result = await measure(format, cursor);
        assert.deepEqual(result.rows, reference.rows);
        if (format === "fury-models") modelBytes = result.body;
        if (format === "fury") assert.deepEqual(result.body, modelBytes);
    }
}
console.log(`Dataset ${process.env.QUIZ_DATASET ?? "quiz"}, ${pageSize} rows/response, max 4 database connections per pool`);
if (orders && pageSize >= 50) {
    const ordersSample = [...references.values()].flat();
    assert(ordersSample.some(row => row.adjustment_cents < 0));
    assert(ordersSample.some(row => row.coupon !== null) && ordersSample.some(row => row.coupon === null));
    assert(ordersSample.some(row => row.note?.length >= 128));
    assert(ordersSample.some(row => /[^\u0000-\u007f]/u.test(row.buyer_name)));
}

function order(index) {
    const start = (index % formats.length + formats.length) % formats.length;
    return [...formats.slice(start), ...formats.slice(0, start)];
}

async function sampleHeap(format, body) {
    const session = new Session();
    session.connect();
    try {
        await session.post("HeapProfiler.startSampling", {
            samplingInterval: 4096, includeObjectsCollectedByMajorGC: true,
            includeObjectsCollectedByMinorGC: true,
        });
        for (let i = 0; i < count; i++) decode[format](body);
        const { profile } = await session.post("HeapProfiler.stopSampling");
        const estimate = profile.samples.reduce((sum, sample) => sum + sample.size, 0) / count;
        console.log(`${format} | sampled V8 heap allocation ~${estimate.toFixed(0)} bytes/decode`);
    } finally {
        session.disconnect();
    }
}

async function load(format, clients) {
    const timings = [];
    let errors = 0;
    let transferred = 0;
    let firstError;
    async function sample(index) {
        const cursor = cursors[index % cursors.length];
        const result = await measure(format, cursor);
        assert.deepEqual(result.rows, references.get(cursor));
        return result;
    }
    await Promise.all(Array.from({length: clients}, (_, index) => sample(index)));
    const start = performance.now();
    const deadline = start + seconds * 1000;
    async function worker(index) {
        while (performance.now() < deadline) {
            try {
                const result = await sample(index++);
                timings.push(result.ms);
                transferred += result.bytes;
            } catch (error) {
                errors++;
                firstError ??= error.message;
            }
        }
    }
    await Promise.all(Array.from({length: clients}, (_, index) => worker(index)));
    const elapsed = (performance.now() - start) / 1000;
    timings.sort((a, b) => a - b);
    const pct = (p) => timings.length ? timings[Math.ceil(timings.length * p) - 1].toFixed(2) : "n/a";
    const rps = (timings.length / elapsed).toFixed(0);
    const rate = (transferred / elapsed / 2**20).toFixed(2);
    const bytes = timings.length ? (transferred / timings.length).toFixed(0) : "n/a";
    console.log(`${format} | ${clients} clients | ${rps} RPS | ${timings.length} ok | ${errors} errors | p50 ${pct(.5)} ms | p95 ${pct(.95)} ms | p99 ${pct(.99)} ms | ${bytes} bytes/response | ${rate} MiB/s`);
    if (errors) {
        console.error(firstError);
        process.exitCode = 1;
    }
}

if (sustained) {
    const levels = (process.env.QUIZ_CLIENTS ?? "1,8,32").split(",").map(Number);
    const repeats = Number(process.env.QUIZ_REPEATS ?? 1);
    assert(levels.length <= 8 && levels.every(n => Number.isInteger(n) && n >= 1 && n <= 128));
    assert(Number.isInteger(repeats) && repeats >= 1 && repeats <= 10);
    for (let repeat = 0; repeat < repeats; repeat++) {
        console.log(`Repeat ${repeat + 1}/${repeats}, ${seconds} seconds per case`);
        for (const [index, clients] of levels.entries()) {
            for (const format of order(index + repeat)) await load(format, clients);
        }
    }
} else {
    const results = Object.fromEntries(formats.map((format) => [format, []]));
    for (let i = -8; i < count; i++) {
        const cursor = cursors[(i % cursors.length + cursors.length) % cursors.length];
        for (const format of order(i)) {
            const result = await measure(format, cursor);
            assert.deepEqual(result.rows, references.get(cursor));
            if (i >= 0) results[format].push(result.ms);
        }
    }
    for (const [format, measured] of Object.entries(results)) {
        const times = measured.sort((a, b) => a - b);
        const percentile = (p) => times[Math.ceil(p * times.length) - 1].toFixed(3);
        const cursor = cursors[0];
        const {body} = await measure(format, cursor);
        const decodeTimes = [];
        for (let i = -8; i < count; i++) {
            const start = performance.now();
            const rows = decode[format](body);
            const elapsed = performance.now() - start;
            assert.deepEqual(rows, references.get(cursor));
            if (i >= 0) decodeTimes.push(elapsed);
        }
        decodeTimes.sort((a, b) => a - b);
        const decodeMedian = decodeTimes[Math.ceil(count / 2) - 1].toFixed(3);
        console.log(`${format} | ${count} requests | ${body.length} bytes | gzip-6 ${gzipSync(body, {level: 6}).length} bytes | p50 ${percentile(.5)} ms | p95 ${percentile(.95)} ms | decode p50 ${decodeMedian} ms`);
        if (process.env.QUIZ_PROFILE === "1") await sampleHeap(format, body);
    }
}
