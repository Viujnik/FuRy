# Use FuRy from JavaScript or TypeScript

A browser build needs the WASM module and the registry generated for your models. Load them together. A `WasmRegistry` belongs to one client, so you can hold separate registries for accounts and shipping or for two versions of an API.

```typescript
import init, { WasmRegistry } from "@fury/wasm-core";

await init();
const accounts = new WasmRegistry(registryBytes);
const users = accounts.decodePage("User", responseBytes, 50);
console.log(users[0]?.id); // 9007199254740993n for that PostgreSQL bigint
```

The scoped reader returns Int64 and UInt64 as `bigint`. It omits a missing field and returns `null` for an explicit null. Supply the maximum number of rows your endpoint promises.

## Open a single record

```typescript
const user = accounts.openRecord("User", frameBytes);
try {
    user.setString("display_name", "Ana Gómez");
    const editedFrame = user.toFinishedBytes();
    console.log(editedFrame.length);
} finally {
    user.free();
}
```

Use `getBigInt` and `setBigInt` when working with exact Int64 or UInt64 fields on a record. JavaScript dates have millisecond precision. The WASM input buffer and objects returned to JavaScript live in different memory. Limit the body size before buffering it. Release a registry with `free()` when its client is finished with it.

Existing applications can load their global registry with `loadRegistry` and use generated classes or global `decodePage`. The global decoder returns small 64-bit integers as numbers and larger ones as bigint. [Code generation](../fury-cli/README.md) · [A complete browser example](../examples/web/INTERNALS.md)
