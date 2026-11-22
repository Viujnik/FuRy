import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { decode, encode } from '@msgpack/msgpack';
import { sha256, splitFuryFrames } from './harness.mjs';

const formats = {
  json: {
    open: (bytes) => JSON.parse(Buffer.from(bytes.buffer, bytes.byteOffset, bytes.byteLength).toString('utf8')),
    encode: (rows) => Buffer.from(JSON.stringify(rows)),
  },
  msgpack: { open: decode, encode },
};
const updatedUsername = 'updated ✓';
export const ALL_VARIANTS = [
  'json_parse_only', 'json_parse_read3', 'json_update_roundtrip',
  'msgpack_decode_only', 'msgpack_decode_read3', 'msgpack_update_roundtrip',
  'fury_construct_only', 'fury_construct_read3', 'fury_update_roundtrip',
];

export async function loadWasm() {
  const path = process.env.FURY_BENCH_WASM;
  if (!path) throw new Error('FURY_BENCH_WASM must point to Node-target fury_ts.js');
  return import(pathToFileURL(resolve(path)).href);
}

function read3(rowCount, fields) {
  let checksum = 0;
  for (let i = 0; i < rowCount; i++) {
    const [id, username, active] = fields(i);
    if (!Number.isSafeInteger(id) || typeof username !== 'string' || typeof active !== 'boolean') {
      throw new Error('Unexpected read3 field types');
    }
    checksum += id + username.length + Number(active);
  }
  return checksum;
}

function fieldsFromFrame(frame) {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const fields = new Map();
  for (let pos = 6; pos < frame.length;) {
    const nameSize = frame[pos++];
    assert.ok(pos + nameSize + 4 <= frame.length);
    const name = Buffer.from(frame.subarray(pos, pos + nameSize)).toString('utf8');
    pos += nameSize;
    const size = view.getUint32(pos, true);
    pos += 4;
    assert.ok(pos + size <= frame.length && !fields.has(name));
    fields.set(name, Buffer.from(frame.subarray(pos, pos + size)));
    pos += size;
  }
  return fields;
}

function compactRows(rows) {
  return rows.map((row) => Object.fromEntries(Object.entries(row).map(([name, value]) =>
    [name, value instanceof Uint8Array ? Buffer.from(value).toString('base64') : value])));
}

export async function prepareVariant(variant, fixtureDir) {
  if (!ALL_VARIANTS.includes(variant)) throw new Error(`Unknown benchmark: ${variant}`);
  const metadata = JSON.parse(await readFile(join(fixtureDir, 'metadata.json'), 'utf8'));
  const { rows, read3_checksum: expectedChecksum, schema_name: schema } = metadata;
  if (metadata.fixture_version !== 2 || metadata.fury_layout !== 'concatenated-v2-frames'
      || !Number.isSafeInteger(rows) || rows < 1 || rows > 10000
      || schema !== 'BenchUser' || metadata.order_by !== 'id ASC'
      || !Number.isSafeInteger(expectedChecksum)) {
    throw new Error('Invalid fixture metadata. Regenerate the fixtures.');
  }
  const verifiedRead = async (name) => {
    const data = await readFile(join(fixtureDir, name));
    if (sha256(data) !== metadata.sha256?.[name]) throw new Error(`Fixture hash mismatch: ${name}`);
    return data;
  };

  const format = variant.split('_')[0];
  const update = variant.endsWith('_roundtrip');
  const read = variant.endsWith('_read3');
  const bytes = await verifiedRead(`wide.${format}`);
  const rowFields = (row) => [row.id, row.username, row.is_active];
  const expected = formats.json.open(await verifiedRead('wide.json'));
  assert.equal(expected.length, rows);
  assert.equal(read3(rows, (i) => rowFields(expected[i])), expectedChecksum);
  let run;
  let verifyRoundtrip;
  let runtimeHash;

  if (format === 'fury') {
    const { loadRegistry, WasmRecord } = await loadWasm();
    runtimeHash = sha256(await readFile(join(resolve(process.env.FURY_BENCH_WASM), '..', 'fury_ts_bg.wasm')));
    loadRegistry(await verifiedRead('.fury_registry.bin'));
    const frames = splitFuryFrames(bytes);
    assert.equal(frames.length, rows);
    const withRecords = (input, fn) => {
      const records = [];
      try {
        for (const frame of input) records.push(new WasmRecord(schema, frame));
        return fn(records);
      } finally {
        for (const record of records) record.free();
      }
    };
    const recordFields = (r) => [r.getNumber('id'), r.getString('username'), r.getBool('is_active')];
    withRecords(frames, (records) => {
      assert.deepEqual(records.map(recordFields), expected.map(rowFields));
    });
    run = () => withRecords(frames, (records) => {
      if (update) {
        return Buffer.concat(records.map((r) => {
          r.setString('username', updatedUsername);
          r.finish();
          return r.toBytes();
        }));
      }
      return { rowCount: records.length, checksum: read ? read3(records.length, (i) => recordFields(records[i])) : null };
    });
    verifyRoundtrip = (output) => {
      const framesAfter = splitFuryFrames(output);
      assert.equal(framesAfter.length, rows);
      withRecords(framesAfter, (records) => {
        assert.deepEqual(records.map(recordFields),
          expected.map((row) => rowFields({ ...row, username: updatedUsername })));
      });
      for (let i = 0; i < rows; i++) {
        const after = fieldsFromFrame(framesAfter[i]);
        const before = fieldsFromFrame(frames[i]);
        assert.equal(after.size, before.size);
        before.delete('username');
        after.delete('username');
        assert.deepEqual(after, before);
      }
    };
  } else {
    const codec = formats[format];
    assert.deepEqual(compactRows(codec.open(bytes)), expected);
    run = () => {
      const output = codec.open(bytes);
      if (update) {
        for (const row of output) row.username = updatedUsername;
        return codec.encode(output);
      }
      return { rowCount: output.length, checksum: read ? read3(output.length, (i) => rowFields(output[i])) : null };
    };
    verifyRoundtrip = (output) => assert.deepEqual(compactRows(codec.open(output)),
      expected.map((row) => ({ ...row, username: updatedUsername })));
  }

  if (update) verifyRoundtrip(run());
  const validate = (result) => {
    if (update) {
      assert.ok(result.length > 0);
    } else {
      assert.equal(result.rowCount, rows);
      if (read) assert.equal(result.checksum, expectedChecksum);
    }
  };
  return {
    run, validate,
    metadata: {
      rows, fixture_sha256: metadata.sha256[`wide.${format}`], corpus_sha256: metadata.sha256['wide.json'],
      fixture_bytes: bytes.length, fixture_version: metadata.fixture_version, order_by: metadata.order_by,
      wasm_sha256: runtimeHash, fields_read_per_row: read ? 3 : 0,
      updates_per_row: update ? 1 : 0,
      wasm_free_timed: format === 'fury', fury_frame_split_timed: false,
      json_utf8_decode_timed: format === 'json',
    },
  };
}
