# elle-polars

A Polars DataFrame plugin for Elle, wrapping the Rust `polars` crate for high-level data manipulation.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_polars.so` (or `target/release/libelle_polars.so`).

## Usage

```lisp
(import-file "path/to/libelle_polars.so")

# Create a DataFrame from column arrays
(def df (polars/df {:name ["Alice" "Bob" "Carol" "Dave"]
                    :age  [30 25 35 28]
                    :dept ["eng" "sales" "eng" "sales"]}))

# Inspect
(polars/shape df)       ;; => [4 3]
(polars/columns df)     ;; => ["age" "dept" "name"]
(polars/dtypes df)      ;; => ["i64" "str" "str"]
(println (polars/display df))

# Extract and slice
(polars/column df "name")         ;; => ["Alice" "Bob" "Carol" "Dave"]
(polars/head df 2)                ;; first 2 rows
(polars/tail df 2)                ;; last 2 rows
(polars/slice df 1 2)             ;; rows at offset 1, length 2

# Select, drop, rename
(polars/select df ["name" "age"])
(polars/drop df ["dept"])
(polars/rename df "dept" "department")

# Sort and deduplicate
(polars/sort df "age")            ;; ascending
(polars/sort df "age" "desc")     ;; descending
(polars/unique df ["dept"])

# Stack DataFrames
(polars/vstack df1 df2)           ;; vertical (same schema)
(polars/hstack df1 df2)           ;; horizontal (add columns)

# Convert to Elle structs
(polars/to-rows df)
;; => [{:age 30 :dept "eng" :name "Alice"} ...]

# CSV round-trip
(def csv-text (polars/write-csv df))
(def df2 (polars/read-csv csv-text))

# Parquet round-trip
(def pq-bytes (polars/write-parquet df))
(def df3 (polars/read-parquet pq-bytes))

# JSON input
(def df4 (polars/read-json "[{\"a\":1},{\"a\":2}]"))
```

### Lazy API

The lazy API defers computation for query optimization:

```lisp
(def result
  (-> df
      polars/lazy
      (polars/lfilter "age" ">" 25)
      (polars/lselect ["name" "age"])
      (polars/lsort "age" "desc")
      polars/collect))

# Group-by with aggregations
(def stats
  (-> df
      polars/lazy
      (polars/lgroupby ["dept"]
        {:avg-age  {:col "age" :agg "mean"}
         :headcount {:col "name" :agg "count"}})
      polars/collect))

# Joins
(def joined
  (-> (polars/lazy left-df)
      (polars/ljoin (polars/lazy right-df) ["id"] "left")
      polars/collect))
```

## Primitives

### Construction / IO

| Name | Args | Returns |
|------|------|---------|
| `polars/df` | columns (struct of arrays) | DataFrame |
| `polars/read-csv` | text | DataFrame |
| `polars/write-csv` | df | string |
| `polars/read-parquet` | bytes | DataFrame |
| `polars/write-parquet` | df | bytes |
| `polars/read-json` | text | DataFrame |

### Inspection

| Name | Args | Returns |
|------|------|---------|
| `polars/shape` | df | [rows, cols] |
| `polars/columns` | df | array of column names |
| `polars/dtypes` | df | array of type strings |
| `polars/head` | df, n? | DataFrame (first n rows, default 5) |
| `polars/tail` | df, n? | DataFrame (last n rows, default 5) |
| `polars/display` | df | formatted table string |
| `polars/to-rows` | df | array of structs |
| `polars/column` | df, name | array of values |
| `polars/describe` | df | array of column stats structs |

### Operations

| Name | Args | Returns |
|------|------|---------|
| `polars/select` | df, columns | DataFrame |
| `polars/drop` | df, columns | DataFrame |
| `polars/rename` | df, from, to | DataFrame |
| `polars/slice` | df, offset, length | DataFrame |
| `polars/sample` | df, n | DataFrame (random sample) |
| `polars/sort` | df, column, order? | DataFrame ("desc" for descending) |
| `polars/unique` | df, columns? | DataFrame |
| `polars/vstack` | df1, df2 | DataFrame (vertical concat) |
| `polars/hstack` | df1, df2 | DataFrame (horizontal concat) |

### Lazy API

| Name | Args | Returns |
|------|------|---------|
| `polars/lazy` | df | LazyFrame |
| `polars/collect` | lazy | DataFrame |
| `polars/lselect` | lazy, columns | LazyFrame |
| `polars/lfilter` | lazy, col, op, value | LazyFrame |
| `polars/lsort` | lazy, column, order? | LazyFrame |
| `polars/lgroupby` | lazy, group-cols, aggs | LazyFrame |
| `polars/ljoin` | left, right, on-cols, how? | LazyFrame |
