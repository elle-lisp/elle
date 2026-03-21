# plugins/arrow

Apache Arrow columnar data via the `arrow` and `parquet` crates.

## Responsibility

Provides low-level columnar data operations for Elle. RecordBatches are the
core abstraction — typed, column-oriented tables with schema metadata. The
plugin handles construction from Elle values, column extraction, schema
inspection, zero-copy slicing, and serialization to/from IPC and Parquet
formats.

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `arrow/batch` | Exact(1) | Create RecordBatch from struct of column arrays |
| `arrow/schema` | Exact(1) | Return schema as struct of column-name → type-string |
| `arrow/num-rows` | Exact(1) | Return row count |
| `arrow/num-cols` | Exact(1) | Return column count |
| `arrow/column` | Exact(2) | Extract a column by name as Elle array |
| `arrow/to-rows` | Exact(1) | Convert batch to array of structs |
| `arrow/display` | Exact(1) | Pretty-print batch as formatted table string |
| `arrow/slice` | Exact(3) | Zero-copy slice: batch, offset, length |
| `arrow/write-ipc` | Exact(1) | Serialize batch to IPC stream bytes |
| `arrow/read-ipc` | Exact(1) | Deserialize IPC bytes to batch(es) |
| `arrow/write-parquet` | Exact(1) | Serialize batch to Parquet bytes |
| `arrow/read-parquet` | Exact(1) | Deserialize Parquet bytes to batch |

## Implementation

RecordBatches are stored as `Value::external("arrow/batch", BatchWrap(..))`.
Type inference maps Elle values to Arrow types: int→Int64, float→Float64,
bool→Boolean, string→Utf8, nil→null. The first non-nil value in a column
determines the column type.

When converting Arrow arrays back to Elle values, numeric types are cast to
Int64 or Float64 as appropriate. Unsupported types fall back to string
formatting via `ArrayFormatter`.

IPC uses Arrow's streaming format (not file format). Parquet read uses the
`bytes` crate for zero-copy buffer handling.

## Building

```bash
cd plugins/arrow
cargo build --release
# Output: target/release/libelle_arrow.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_arrow.so"))
(def batch (arrow/batch {:x [1 2 3] :y [4.0 5.0 6.0]}))
(println (arrow/display batch))
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
