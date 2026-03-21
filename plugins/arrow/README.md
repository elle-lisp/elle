# elle-arrow

An Apache Arrow plugin for Elle, wrapping the Rust `arrow` and `parquet` crates for columnar data processing.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_arrow.so` (or `target/release/libelle_arrow.so`).

## Usage

```lisp
(import-file "path/to/libelle_arrow.so")

# Create a RecordBatch from column arrays
(def batch (arrow/batch {:name ["Alice" "Bob" "Carol"]
                         :age  [30 25 35]}))

# Inspect
(arrow/num-rows batch)   ;; => 3
(arrow/num-cols batch)   ;; => 2
(arrow/schema batch)     ;; => {:age "Int64" :name "Utf8"}

# Extract a column
(arrow/column batch "name")  ;; => ["Alice" "Bob" "Carol"]

# Convert to Elle structs
(arrow/to-rows batch)
;; => [{:age 30 :name "Alice"} {:age 25 :name "Bob"} {:age 35 :name "Carol"}]

# Pretty-print
(println (arrow/display batch))

# Zero-copy slicing
(def first-two (arrow/slice batch 0 2))

# Serialize to IPC and back
(def ipc-bytes (arrow/write-ipc batch))
(def batch2 (arrow/read-ipc ipc-bytes))

# Serialize to Parquet and back
(def pq-bytes (arrow/write-parquet batch))
(def batch3 (arrow/read-parquet pq-bytes))
```

## Primitives

| Name | Args | Returns |
|------|------|---------|
| `arrow/batch` | columns (struct of arrays) | RecordBatch |
| `arrow/schema` | batch | struct of column-name → type-string |
| `arrow/num-rows` | batch | integer |
| `arrow/num-cols` | batch | integer |
| `arrow/column` | batch, column-name | array of values |
| `arrow/to-rows` | batch | array of structs |
| `arrow/display` | batch | formatted table string |
| `arrow/slice` | batch, offset, length | RecordBatch (zero-copy) |
| `arrow/write-ipc` | batch | bytes (IPC stream format) |
| `arrow/read-ipc` | bytes | RecordBatch or array of batches |
| `arrow/write-parquet` | batch | bytes (Parquet format) |
| `arrow/read-parquet` | bytes | RecordBatch |

## Type Mapping

| Elle type | Arrow type |
|-----------|------------|
| integer | Int64 |
| float | Float64 |
| boolean | Boolean |
| string | Utf8 |
| nil | null |
