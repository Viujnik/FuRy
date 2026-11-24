const assert = require("node:assert/strict");
const {readFileSync, writeFileSync} = require("node:fs");
const {decodePage, loadRegistry, WasmRecord, WasmRegistry} = require("@fury/wasm-core");
const {Message} = require("./js/out/message");
const {Summary} = require("./js/out/summary");
const {update, count} = require("./js/consumer");

const blob = readFileSync("out/.fury_registry.bin");
loadRegistry(blob);
assert.throws(() => loadRegistry(blob.subarray(0, -1)));

const oversized = new Uint8Array(16 * 1024 * 1024 + 1);
assert.throws(() => new WasmRecord("Message", oversized), /byte limit/);
assert.throws(() => loadRegistry(oversized), /byte limit/);

const original = new WasmRecord("Message", readFileSync("golden.fury"));
try {
    loadRegistry(readFileSync("replacement.registry"));
    assert.throws(() => loadRegistry(blob.subarray(0, -1)));
    const replacement = new WasmRecord("Message");
    try {
        replacement.setNumber("count", 250);
        assert.equal(replacement.getNumber("count"), 250);
        assert.throws(() => replacement.setNumber("id", 1));
        assert.equal(original.getBigInt("id"), 42n);
    } finally {
        replacement.free();
    }
} finally {
    original.free();
    loadRegistry(blob);
}

const pageFrames = Buffer.concat([readFileSync("golden.fury"), readFileSync("message.bin")]);
const scoped = new WasmRegistry(blob);
const separate = new WasmRegistry(readFileSync("unsigned.registry"));
assert.throws(() => separate.decodePage("Message", pageFrames, 2));
assert.deepEqual(scoped.decodePage("Message", pageFrames, 2), [
    {id: 42n},
    {id: 9007199254740993n, text: "Café", active: true, note: null},
]);
const scopedRecord = scoped.openRecord("Message", readFileSync("golden.fury"));
assert.equal(scopedRecord.getBigInt("id"), 42n);
scopedRecord.free();
separate.free();
assert.deepEqual(decodePage("Message", pageFrames, 2), [
    {id: 42},
    {id: 9007199254740993n, text: "Café", active: true, note: null},
]);
assert.deepEqual(decodePage("Message", new Uint8Array(), 0), []);
assert.throws(() => decodePage("Message", pageFrames, 1));
assert.throws(() => decodePage("Message", pageFrames.subarray(0, -1), 2));
assert.throws(() => decodePage("Message", pageFrames, -1));
assert.throws(() => decodePage("Message", new Uint8Array(16 * 1024 * 1024 + 1), 1));
Object.defineProperty(Object.prototype, "id", {
    configurable: true,
    set() { throw new Error("unexpected inherited setter"); },
});
try {
    assert.deepEqual(decodePage("Message", readFileSync("golden.fury"), 1), [{id: 42}]);
} finally {
    delete Object.prototype.id;
}

scoped.free();

const golden = new Message(readFileSync("golden.fury"));
try {
    assert.equal(golden.id, 42);
    assert.throws(() => golden.note);
    assert.deepEqual(golden.toBytes(), new Uint8Array(readFileSync("golden.fury")));
    for (const value of [-(1n << 63n), (1n << 63n) - 1n]) {
        golden.$record.setBigInt("id", value);
        assert.equal(golden.$record.getBigInt("id"), value);
    }
} finally {
    golden.$record.free();
}

const message = new Message(readFileSync("message.bin"));
try {
    assert.equal(message.text, "Café");
    assert.equal(message.active, true);
    assert.throws(() => message.id);
    assert.throws(() => { message.id = 1.5; });
    assert.throws(() => { message.id = Number.MAX_SAFE_INTEGER + 1; });
    assert.throws(() => message.$record.setBigInt("id", 1n << 63n));
    assert.throws(() => message.$record.setBigInt("id", 2));
    assert.throws(() => message.$record.setNull("id"));
    assert.deepEqual(message.$record.toBytes(), new Uint8Array(readFileSync("message.bin")));
    update(message);
    writeFileSync("updated.bin", message.toBytes());
} finally {
    message.$record.free();
}

const summary = new Summary();
try {
    summary.count = 42;
    assert.equal(count(summary), 42);
    assert.equal(summary.label, null);
    const bytes = summary.toBytes();
    assert.deepEqual(summary.toBytes(), bytes);
    const copy = new Summary(bytes);
    try { assert.equal(copy.count, 42); } finally { copy.$record.free(); }
} finally {
    summary.$record.free();
}

loadRegistry(readFileSync("unsigned.registry"));
const unsigned = new WasmRecord("Unsigned");
try {
    const max = (1n << 64n) - 1n;
    unsigned.setBigInt("total", max);
    unsigned.setNumber("count", 255);
    unsigned.finish();
    assert.throws(() => unsigned.setNumber("count", 256));
    assert.throws(() => unsigned.setBigInt("total", max + 1n));
    assert.throws(() => unsigned.setBigInt("total", -1n));
    assert.equal(unsigned.getBigInt("total"), max);
    assert.equal(unsigned.getNumber("count"), 255);
} finally {
    unsigned.free();
}
