import assert from "node:assert/strict";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const { fetchLeaderboard } = await import(pathToFileURL(resolve(process.argv[2], "wire.js")));
const result = [{handle: "demo-001", score: 3n}];
const registry = {decodePage(schema, bytes, maxRows) {
    assert.equal(schema, "LeaderboardEntry");
    assert.equal(maxRows, 128);
    assert.deepEqual([...bytes], [1, 2, 3]);
    return result;
}};
const requested = [];
assert.deepEqual(await fetchLeaderboard(registry, async url => {
    requested.push(url);
    return new Response(new Uint8Array([1, 2, 3]));
}), result);
assert.deepEqual(requested, ["/api/leaderboard.fury"]);
await assert.rejects(fetchLeaderboard(registry, async () => new Response(null, {status: 503})), /503/);
