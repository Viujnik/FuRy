import init, { loadRegistry } from "@fury/wasm-core";
import { Item } from "./models/item.js";

export interface Row {
    id: string;
    title: string;
    available: boolean;
    note: string | null;
}

async function getBytes(origin: URL, path: string): Promise<Uint8Array> {
    const response = await fetch(new URL(path, origin));
    if (!response.ok) {
        throw new Error(`Could not load ${path} (HTTP ${response.status})`);
    }
    return new Uint8Array(await response.arrayBuffer());
}

export async function loadRows(origin: URL): Promise<Row[]> {
    await init({ module_or_path: new URL("/wasm/fury_ts_bg.wasm", origin) });
    loadRegistry(await getBytes(origin, "/registry.bin"));
    const bytes = await getBytes(origin, "/api/items");
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);

    // The example response contains complete FuRy frames concatenated in row order.
    const rows: Row[] = [];
    for (let start = 0; start < bytes.length;) {
        if (bytes.length - start < 6 || rows.length === 3) {
            throw new Error("Unexpected demo page length");
        }
        const frameSize = view.getUint32(start, true);
        if (frameSize < 6 || frameSize > bytes.length - start) {
            throw new Error("Unexpected record length");
        }
        const end = start + frameSize;
        const item = new Item(bytes.subarray(start, end));
        try {
            const id = item.$record.getBigInt("id");
            if (id === null) {
                throw new Error("Demo record is missing an id");
            }
            rows.push({ id: id.toString(), title: item.title, available: item.available, note: item.note });
        } finally {
            item.$record.free();
        }
        start = end;
    }
    return rows;
}
