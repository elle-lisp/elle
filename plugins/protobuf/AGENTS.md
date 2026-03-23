# plugins/protobuf

Protocol Buffers encoding, decoding, and introspection for Elle.

## Responsibility

Provides dynamic protobuf encode/decode without code generation. Users load
`.proto` source text (or a pre-compiled binary `FileDescriptorSet`) at runtime
to produce an opaque descriptor pool. Encode and decode operate against message
descriptors from that pool.

This is the right approach for a scripting language: no hand-translating
`.proto` files into Elle DSL, no pre-build step, full ecosystem compatibility.

## Primitives

| Name | Arity | Signal | Purpose |
|------|-------|--------|---------|
| `protobuf/schema` | 1–2 | errors | Parse `.proto` text → descriptor pool |
| `protobuf/schema-bytes` | 1 | errors | Load binary `FileDescriptorSet` → pool |
| `protobuf/encode` | 3 | errors | Elle struct → protobuf bytes |
| `protobuf/decode` | 3 | errors | Protobuf bytes → Elle struct |
| `protobuf/messages` | 1 | errors | List fully-qualified message names in pool |
| `protobuf/fields` | 2 | errors | List fields of a message as structs |
| `protobuf/enums` | 1 | errors | List enum types in pool as structs |

The plugin returns a struct from `elle_plugin_init` mapping short names to
native functions. Both `(protobuf/encode ...)` (after loading) and
`(get plugin :encode)` forms work.

## Value Mapping

### Protobuf → Elle (decode)

| Protobuf type | Elle type | Notes |
|---------------|-----------|-------|
| `int32`, `sint32`, `sfixed32` | `int` | Direct |
| `int64`, `sint64`, `sfixed64` | `int` | Direct |
| `uint32`, `fixed32` | `int` | Max 4,294,967,295 — fits in Elle int |
| `uint64`, `fixed64` | `int` | Error if > 2⁶³−1 (Elle i64 signed range) |
| `float` | `float` | Widened to f64 |
| `double` | `float` | Direct |
| `bool` | `bool` | Direct |
| `string` | `string` | Immutable |
| `bytes` | `bytes` | Immutable |
| `enum` | `keyword` | Value name as keyword: `STATUS_OK` → `:STATUS_OK` |
| message | `struct` | Immutable struct with keyword keys, recursive |
| `repeated T` | `array` | Immutable array of decoded elements |
| `map<K, V>` | `struct` | Immutable struct; string keys → keyword keys |
| unset field | absent | Field omitted from struct (not `nil`) |
| unknown enum number | `int` | Forward-compatibility: number returned as int |

### Elle → Protobuf (encode)

| Elle type | Protobuf type | Notes |
|-----------|---------------|-------|
| `int` | `int32`/`int64`/`uint32`/`uint64`/etc. | Range-checked against field descriptor |
| `float` | `float`/`double` | Narrowed to f32 if field is `float` |
| `bool` | `bool` | Direct |
| `string` / `@string` | `string` | |
| `bytes` / `@bytes` | `bytes` | |
| `keyword` | `enum` | Keyword name matched against enum descriptor |
| `struct` / `@struct` | message | Recursive; keyword keys matched to field names |
| `array` / `@array` | `repeated` | Each element encoded per field type |
| `nil` | (field omitted) | Nil means field not set |

Non-keyword keys in Elle structs (int, string, bool) are silently ignored
during encode. This avoids errors when a struct carries extra metadata keys.

### uint64/fixed64 map key limitation

`uint64` and `fixed64` map keys can hold values 0–2⁶⁴−1. Elle's `int` is
i64 signed. Values above 2⁶³−1 are represented as **string keys** in the
decoded struct (decimal representation). Encode accepts either int or string
keys for uint64/fixed64 map fields.

All affected encode/decode paths are marked:
```rust
// TODO(uint64): Elle has no u64 Value type; using string representation
// for uint64/fixed64 map keys > i64::MAX.
```

## Implementation Notes

### Two-crate approach

This plugin uses two protobuf Rust implementations that coexist:

- **`protobuf` + `protobuf-parse`** (3.x): Pure-Rust `.proto` text parser.
  `protobuf-parse` has no in-memory string API — it requires actual files on
  disk. `schema.rs` writes the proto source to a temp directory via `tempfile`.

- **`prost-reflect`** (0.14): Dynamic message encode/decode via
  `DescriptorPool` and `DynamicMessage`. This is the runtime layer.

The bridge: serialize the `FileDescriptorSet` (protobuf crate) to bytes with
`protobuf::Message::write_to_bytes()`, then load into `prost-reflect` with
`DescriptorPool::decode()`. One-time cost at schema load time.

### parse_and_typecheck vs file_descriptor_set

`schema.rs` uses `Parser::parse_and_typecheck()`, NOT `.file_descriptor_set()`.
The latter strips transitive dependency files from the result, breaking
cross-file type resolution when one `.proto` imports another. `parse_and_typecheck`
includes all files (input + dependencies) in the returned `ParsedAndTypechecked`.

### Temp file requirement

`protobuf-parse` requires `.proto` source on disk (no in-memory string API).
The schema loader creates a `tempfile::tempdir()`, writes the proto text, and
passes the path to the parser. The `tempfile` crate handles cleanup.

### External value

The descriptor pool is stored as:
```rust
Value::external("protobuf/pool", pool)
```
where `pool` is a `prost_reflect::DescriptorPool`. Retrieved with
`val.as_external::<DescriptorPool>()`.

## Building

```bash
# Debug (note: debug .so cannot be loaded on some Linux systems due to TLS)
cargo build -p elle-protobuf
# Output: target/debug/libelle_protobuf.so

# Release
cargo build --release -p elle-protobuf
# Output: target/release/libelle_protobuf.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_protobuf.so"))
(def schema-fn   (get plugin :schema))
(def encode-fn   (get plugin :encode))
(def decode-fn   (get plugin :decode))
(def messages-fn (get plugin :messages))
(def fields-fn   (get plugin :fields))
(def enums-fn    (get plugin :enums))

(def pool (schema-fn "syntax = \"proto3\"; message Foo { string x = 1; }"))
(def buf  (encode-fn pool "Foo" {:x "hello"}))
(decode-fn pool "Foo" buf)  # => {:x "hello"}
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `plan.md` | Full API specification, design rationale, implementation chunks |
| `src/lib.rs` | Plugin entry point, primitive registration, `elle_plugin_init` |
| `src/schema.rs` | Schema loading: `protobuf/schema`, `protobuf/schema-bytes`, shared helpers |
| `src/convert.rs` | Value conversion: `protobuf/encode`, `protobuf/decode`, `elle_to_pb`, `pb_to_elle` |
| `src/inspect.rs` | Introspection: `protobuf/messages`, `protobuf/fields`, `protobuf/enums` |
