import assert from "node:assert/strict";
import {copyFile, readFile} from "node:fs/promises";
import {join} from "node:path";
import {pathToFileURL} from "node:url";

// Evaluate the browser glue as ESM with the WASM bytes supplied by the host.
const directory = process.argv[2];
const glue = join(directory, "smoke.mjs");
await copyFile(join(directory, "fury_ts.js"), glue);
const wasm = await import(pathToFileURL(glue));
await wasm.default({module_or_path: await readFile(join(directory, "fury_ts_bg.wasm"))});
wasm.loadRegistry(await readFile("out/.fury_registry.bin"));
const record = new wasm.WasmRecord("Message", await readFile("golden.fury"));
try {
    assert.equal(record.getBigInt("id"), 42n);
} finally {
    record.free();
}
