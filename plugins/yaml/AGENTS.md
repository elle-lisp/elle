# plugins/yaml

YAML parsing and serialization for Elle.

## Responsibility

Provides three primitives for working with YAML files: `yaml/parse` converts
the first document in a YAML string into Elle values, `yaml/parse-all` parses
all documents (multi-document YAML), and `yaml/encode` converts Elle values
back into a YAML string. Uses the `serde_yml` crate as the underlying
implementation.

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `yaml/parse` | Exact(1) | Parse a YAML string (first document). Returns a struct or value. |
| `yaml/parse-all` | Exact(1) | Parse all YAML documents. Returns an array of values. |
| `yaml/encode` | Exact(1) | Encode an Elle value to a YAML string. |

## Type Mapping

### YAML → Elle (`yaml/parse`, `yaml/parse-all`)

| YAML type | Elle type | Notes |
|-----------|-----------|-------|
| String | string | Direct |
| Integer | int | Direct |
| Float | float | Direct |
| Boolean | bool | Direct |
| Null (`null`, `~`, `!!null`) | nil | `Value::NIL` |
| Sequence | array (immutable) | Recursive |
| Mapping | struct (immutable, keyword keys) | Recursive; non-string keys → error |
| Tagged | (unwrap tag, convert value) | Tag is discarded |

### Elle → YAML (`yaml/encode`)

| Elle type | YAML type | Notes |
|-----------|-----------|-------|
| string | String | |
| int | Integer | |
| float | Float | |
| bool | Boolean | |
| nil | Null | YAML supports null (unlike TOML) |
| array / @array | Sequence | Recursive |
| struct / @struct | Mapping | Keyword keys become string keys |
| other | — | Error: unsupported type |

## Implementation

Uses `serde_yml = "0.0.12"` (maintained fork of the deprecated `serde_yaml`).

- `yaml/parse` calls `serde_yml::from_str::<serde_yml::Value>()`, then
  recursively converts to Elle `Value`.
- `yaml/parse-all` uses `serde_yml::Deserializer::from_str()` to iterate all
  documents, converting each to an Elle `Value`. Documents are separated by
  `---`.
- `yaml/encode` recursively converts Elle `Value` to `serde_yml::Value`, then
  serializes with `serde_yml::to_string()`.

Parsed structs are immutable (`Value::struct_from`). Parsed arrays are
immutable (`Value::array`). Config data is read-only by convention.

## Building

```bash
cargo build --release -p elle-yaml
# Output: target/release/libelle_yaml.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_yaml.so"))
(def parse-fn     (get plugin :parse))
(def parse-all-fn (get plugin :parse-all))
(def encode-fn    (get plugin :encode))
(parse-fn "name: hello\nversion: 1")
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
