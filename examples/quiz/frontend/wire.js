import { decodePage } from "@fury/wasm-core";
import { LeaderboardEntry } from "./models/leaderboardentry.js";

export function readModelRows(bytes, maxRows, Model, project) {
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const rows = [];
    for (let offset = 0; offset < bytes.length;) {
        if (rows.length >= maxRows || bytes.length - offset < 6) {
            throw new Error("Invalid FuRy page");
        }
        const size = view.getUint32(offset, true);
        if (size < 6 || size > bytes.length - offset) {
            throw new Error("Invalid FuRy frame");
        }
        const entry = new Model(bytes.subarray(offset, offset + size));
        try {
            rows.push(project(entry));
        } finally {
            entry.$record.free();
        }
        offset += size;
    }
    return rows;
}

export function readLeaderboard(bytes, maxRows = 128) {
    return readModelRows(bytes, maxRows, LeaderboardEntry, entry => ({handle: entry.handle, score: entry.score}));
}

export function readDirectLeaderboard(bytes, maxRows = 128) {
    return decodePage("LeaderboardEntry", bytes, maxRows);
}

export async function fetchLeaderboard(registry, request = fetch) {
    const response = await request("/api/leaderboard.fury");
    if (!response.ok) throw new Error(`Leaderboard request failed (${response.status})`);
    return registry.decodePage("LeaderboardEntry", new Uint8Array(await response.arrayBuffer()), 128);
}
