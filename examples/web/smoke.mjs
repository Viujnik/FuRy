import assert from "node:assert/strict";
import { loadRows } from "./records.js";

assert.deepEqual(await loadRows(new URL(process.argv[2])), [
    { id: "9007199254740993", title: "Café beans", available: true, note: null },
    { id: "9007199254740994", title: "Oat milk", available: true, note: "Chilled" },
    { id: "9007199254740995", title: "Chocolate", available: false, note: "Restocking" },
]);
