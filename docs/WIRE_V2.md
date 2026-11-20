# FuRy wire format v2

The source of truth is [the Rust encoder and decoder](../fury-core/src/encoder). Multi-byte numeric values are little-endian.

```text
u32 total_bytes | 0x46 | 0x02 | field*
field = u8 name_bytes | name_utf8 | u32 value_bytes | value
```

The total includes the six-byte header. The reader needs the model schema. The transport carries the model identity and frames multiple records as needed.

A value begins with `0x00` for explicit null, or `0x01` followed by the typed payload. Signed and unsigned integers have fixed widths. Float32 and Float64 use IEEE 754. A boolean uses one byte, 0 or 1. Dates are signed i32 days since 1970-01-01. DateTimes are signed i64 microseconds since the Unix epoch. UUIDs use 16 raw bytes. Strings, bytes and opaque objects carry a u32 byte count and their contents. A list carries a u32 count then flagged values. A map carries a u32 pair count, then flagged keys and values.

Projection can omit a field. A present null requires a nullable field. Additions with new field names are compatible when older readers preserve the unknown bytes during edits. Renames and changes to field type or nullability require coordinated migration. Valid legacy duplicates use the last value. Edits can change the order of fields. Compare records by name and value.

The bincode schema registry is a distinct artifact. Its representation depends on matching builds and schema enum layouts.

The default decoder allows a 16 MiB frame, 4,096 fields, 100,000 items per collection and 100,000 known values in the frame. The container depth ceiling is 16. Callers in Rust can select budgets with `DecodeLimits`. Limit incoming bytes before the transport or binding allocates them.
