# fury-core

The Rust crate provides schema definitions, the wire-v2 codec, and an optional PostgreSQL adapter. Disable default features when you only need schemas and records.

```sh
cargo run --locked --no-default-features -p fury-core --example roundtrip
cargo doc --locked --open --no-deps -p fury-core
```

## Record lifecycle

Define a `ModelSchema` with named, typed fields and top-level nullability. Create a `BinaryRecord`, write fields, and call `finish()` before exporting bytes. `from_raw_bytes(Vec<u8>, &schema)` takes ownership of a complete frame, validates known fields, and indexes offsets without materializing their values.

`get_field_bytes` borrows a known field's encoded payload from the frame. `get_field_value` decodes it into an owned value. Use `update_field` to edit a finished record, then call `finish()` again. Edits preserve retained unknown fields and can compact the frame into a new buffer. Language bindings and the HTTP demo copy data at their own boundaries.

## Wire v2

Multi-byte integers and numeric payloads use little-endian byte order. The [codec](src/encoder) and [golden fixtures](tests/fixtures) are the reference implementation and test vectors.

```text
frame = u32 total_bytes | 0x46 | 0x02 | field*
field = u8 name_bytes | UTF-8 name | u32 value_bytes | value
```

The length includes all six header bytes. Field lengths count the presence flag. The frame contains field names but carries neither type tags nor a model ID. The caller supplies the schema and determines how messages are framed over the transport.

Each known value begins with `0x00` for explicit null or `0x01` followed by the payload below. Null has no payload after the flag.

| Schema type | Payload for a present value |
| --- | --- |
| Signed or unsigned integer | Fixed width of 1, 2, 4, or 8 bytes, using two's complement for signed values |
| Float32 or Float64 | IEEE 754 binary32 or binary64 |
| Bool | One byte, `0` or `1` |
| Date | Signed i32 days since 1970-01-01 |
| DateTime | Signed i64 microseconds since the Unix epoch, interpreted as UTC |
| UUID | 16 raw bytes |
| String | u32 byte count, followed by UTF-8 bytes |
| Bytes or Object | u32 byte count, followed by opaque bytes |
| List | u32 element count, followed by flagged elements |
| Map | u32 pair count, followed by a flagged key and value per pair |

Top-level null requires a nullable field. A missing field is allowed and remains distinct from a present null. Collection elements carry their own presence flags. The core treats embedded Object bytes as opaque. For validator behavior and temporal range, follow the Rust tests and implementation.

Adding fields with new names permits older readers to retain those bytes through edits. Renaming a field or changing its type or nullability requires a coordinated schema migration. Valid legacy duplicates use the last value, and edits may reorder fields. The version byte applies to the record layout. The separate bincode registry depends on the schema enum layout, so build it together with the matching runtime.

The default `DecodeLimits` allow 16 MiB per frame, 4,096 physical fields, 100,000 items per collection and 100,000 values for fields known to the reader. The hard nesting limit is 16. Limit input at the transport before reading or allocating the body and enforce application-specific bounds on rows and messages.

## Integrating a driver or language

The `db` feature exposes a PostgreSQL adapter. It uses named columns and bound SQL arguments. Supported projections cover scalar types. The application owns query design, result limits, transaction strategy and transport. See the [Python binding](../fury-py/) for its narrower public API.

For another database, map its typed result rows to `ValueType` and keep connection handling, null mapping, pagination and errors in the adapter crate. For another language, bind to Rust or implement this wire format. Use the golden fixtures in both directions and check integer range, byte order, null and missing fields, bounds and preservation of unknown fields. See the [workspace README](../README.md) for installation and the browser demo.
