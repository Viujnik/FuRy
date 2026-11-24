# fury-core

The Rust crate provides schema definitions, the wire codec, and an optional PostgreSQL adapter. Disable default features when you only need schemas and records.

```sh
cargo run --locked --no-default-features -p fury-core --example roundtrip
cargo doc --locked --open --no-deps -p fury-core
```

## Record lifecycle

Define a `ModelSchema` with named, typed fields and top-level nullability. Create a `BinaryRecord`, write fields, and call `finish()` before exporting bytes. `from_raw_bytes(Vec<u8>, &schema)` takes ownership of a complete frame, validates known fields, and indexes offsets without materializing their values.

`get_field_bytes` borrows a known field's encoded payload from the frame. `get_field_value` decodes it into an owned value. Use `update_field` to edit a finished record, then call `finish()` again. Edits preserve retained unknown fields and can compact the frame into a new buffer. Language bindings and the HTTP demo copy data at their own boundaries.

## Wire format

A complete record containing only `id: Int64 = 42` looks like this:

```text
0b 00 00 00 | 46 03 | 02 69 64 | 01 | 56
```

The first four bytes give its total size, 11. Next come the letter F, version 3, the length and bytes of the name `id`, the payload length, and the encoded integer. A payload of 255 bytes or more uses the length marker 255 followed by a little-endian u32. A page is a sequence of these frames. An empty page occupies zero bytes.

The caller supplies the schema. Marker 0 means null and 1 introduces a full-width value. Integers have shorter forms. Signed values use ZigZag (`0 → 0`, `−1 → 1`, `1 → 2`). A magnitude from 0 through 251 fits directly in one marker, magnitude plus two. Markers 254 and 255 introduce two or four little-endian bytes of the unsigned or ZigZag value when this saves space. Otherwise the writer uses marker 1 and the original integer at its declared width. Thus 42 uses one byte and the largest Int64 uses nine including its marker. For strings, bytes and opaque objects, markers 2 through 255 encode byte lengths through 253. Longer values use marker 1 and a u32 length. Lists and maps use a u32 count and individually marked elements. Date counts days, DateTime counts microseconds, and UUID occupies 16 payload bytes.

The reader accepts v2 frames. An untouched record retains the original bytes. After editing, the writer emits v3 and retains unknown fields. Upgrade readers before enabling the new writer. The experimental FP page from the development branch has been removed. A change to an existing field’s type needs a coordinated schema migration. [Frame parser](src/encoder/frame.rs) · [Value encoding](src/encoder/value.rs) · [Compatibility fixtures and checks](tests/wire_contract.rs)

The default limits are 16 MiB per frame, 4,096 fields, 100,000 values and a nesting depth of 16. Bound the incoming HTTP body before buffering it.
