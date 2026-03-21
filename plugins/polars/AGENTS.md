# plugins/polars

High-level DataFrame operations via the `polars` crate.

## Responsibility

Provides a DataFrame abstraction for Elle — columnar data with schema,
selection, filtering, sorting, grouping, joining, and serialization. Includes
both an eager API (operates on DataFrames directly) and a lazy API (builds a
query plan for optimized execution).

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `polars/df` | Exact(1) | Create DataFrame from struct of column arrays |
| `polars/read-csv` | Exact(1) | Parse CSV text into DataFrame |
| `polars/write-csv` | Exact(1) | Serialize DataFrame to CSV text |
| `polars/read-parquet` | Exact(1) | Read Parquet bytes into DataFrame |
| `polars/write-parquet` | Exact(1) | Serialize DataFrame to Parquet bytes |
| `polars/read-json` | Exact(1) | Parse JSON text into DataFrame |
| `polars/shape` | Exact(1) | Return [rows, cols] dimensions |
| `polars/columns` | Exact(1) | Return column names as string array |
| `polars/dtypes` | Exact(1) | Return column types as string array |
| `polars/head` | Range(1,2) | First n rows (default 5) |
| `polars/tail` | Range(1,2) | Last n rows (default 5) |
| `polars/display` | Exact(1) | Pretty-print DataFrame as table string |
| `polars/to-rows` | Exact(1) | Convert DataFrame to array of structs |
| `polars/column` | Exact(2) | Extract single column as Elle array |
| `polars/describe` | Exact(1) | Column stats (name, dtype, count, null_count) |
| `polars/select` | Exact(2) | Select columns by name |
| `polars/drop` | Exact(2) | Drop columns by name |
| `polars/rename` | Exact(3) | Rename a column |
| `polars/slice` | Exact(3) | Row slice: offset, length |
| `polars/sample` | Exact(2) | Random sample of n rows |
| `polars/sort` | Range(2,3) | Sort by column, optional "desc" |
| `polars/unique` | Range(1,2) | Unique rows, optional column subset |
| `polars/vstack` | Exact(2) | Vertical concatenation |
| `polars/hstack` | Exact(2) | Horizontal concatenation |
| `polars/lazy` | Exact(1) | Convert DataFrame to LazyFrame |
| `polars/collect` | Exact(1) | Execute LazyFrame, return DataFrame |
| `polars/lselect` | Exact(2) | Lazy select columns |
| `polars/lfilter` | Exact(4) | Lazy filter: col, op, value |
| `polars/lsort` | Range(2,3) | Lazy sort by column |
| `polars/lgroupby` | Exact(3) | Lazy group-by with aggregation specs |
| `polars/ljoin` | Range(3,4) | Lazy join: inner/left/full/cross |

## Implementation

DataFrames are stored as `Value::external("polars/df", DfWrap(..))`.
LazyFrames are stored as `Value::external("polars/lazy", LazyWrap(..))`.

Type inference maps Elle values to Polars types: int→i64, float→f64,
bool→Boolean, string→String, nil→null. The first non-nil value in a column
determines the column type.

The lazy API builds Polars expression trees. `lfilter` accepts comparison
operators as strings ("=", "!=", "<", ">", "<=", ">="). `lgroupby` accepts
aggregation specs as structs with `:col` and `:agg` keys, where `:agg` is one
of "sum", "mean", "min", "max", "count", "first", "last".

## Building

```bash
cd plugins/polars
cargo build --release
# Output: target/release/libelle_polars.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_polars.so"))
(def df (polars/df {:x [1 2 3] :y ["a" "b" "c"]}))
(println (polars/display df))
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
