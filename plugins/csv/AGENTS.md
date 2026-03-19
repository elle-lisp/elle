# plugins/csv

CSV parsing and serialization via the `csv` crate.

## Responsibility

Provides CSV read and write operations for Elle. Parsing with headers maps
each row to a keyword-keyed struct; parsing without headers returns arrays of
strings. Writing accepts arrays of structs or arrays of arrays. An optional
`{:delimiter char-string}` opts argument allows non-comma delimiters (e.g.
tab-separated values).

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `csv/parse` | Range(1,2) | Parse CSV string with headers → array of structs |
| `csv/parse-rows` | Range(1,2) | Parse CSV string without headers → array of arrays |
| `csv/write` | Range(1,2) | Serialize array of structs → CSV string |
| `csv/write-rows` | Range(1,2) | Serialize array of arrays → CSV string |

## Implementation

Uses the `csv` crate (version 1) for both reading and writing. All CSV values
are represented as strings — no type inference is performed on parse. The
optional second argument is a struct with a `:delimiter` key whose value must
be a single-character string.

Keys in parsed structs are `TableKey::Keyword` entries matching the CSV header
names. When writing, keys are extracted from the first struct in BTreeMap
(alphabetical) order and used as the header row; subsequent rows are written
in the same key order.

## Building

```bash
cd plugins/csv
cargo build --release
# Output: target/release/libelle_csv.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_csv.so"))
(def parse-fn (get plugin :parse))
(parse-fn "name,age\nAlice,30")
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
