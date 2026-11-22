# fury-ts

The WebAssembly binding opens FuRy frames from JavaScript. Use the [workspace demo](../examples/web/) for a complete build and a real PostgreSQL response, or consult the [wire contract](../fury-core/#wire-v2) for details.

1. Load the WASM package. For a web build, await its initialization function.
2. Load the matching schema artifact as a `Uint8Array` and pass it to `loadRegistry`.
3. Create a generated class or `WasmRecord` for one complete frame. Release the record with `free()` when done. Generated classes expose this method on `$record`.

Generated classes expose numeric, boolean, string, byte-array and date properties. A nullable field can return or accept `null`. Use `$record.getBigInt(name)` and `$record.setBigInt(name, value)` for exact Int64/UInt64 values outside JavaScript's safe integer range. Numeric accessors reject out-of-range integers. JavaScript `Date` has millisecond precision. Generated classes omit UUID and composite properties.

A generated class's `toBytes()` returns a complete frame, finalizing edits when needed. For low-level writes, use `WasmRecord.toFinishedBytes()` before sending the frame. WASM input and JavaScript output use distinct buffers. Bound input at the application transport before buffering it.
