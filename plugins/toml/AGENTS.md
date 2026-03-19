# plugins/toml

TOML parsing and serialization for Elle.

## Responsibility

Provides two primitives for working with TOML configuration files: `toml/parse`
converts a TOML string into Elle values, and `toml/encode` converts Elle values
back into a TOML string. Uses the `toml` crate as the underlying implementation.

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `toml/parse` | Exact(1) | Parse a TOML string. Returns a struct with keyword keys. |
| `toml/encode` | Exact(1) | Encode an Elle value to a TOML string. |

## Type Mapping

### TOML → Elle (`toml/parse`)

| TOML type | Elle type | Notes |
|-----------|-----------|-------|
| String | string | Direct |
| Integer | int | Direct |
| Float | float | Direct |
| Boolean | bool | Direct |
| Array | array (immutable) | Recursive |
| Table | struct (immutable, keyword keys) | Recursive |
| Datetime | string | ISO 8601 text representation |

### Elle → TOML (`toml/encode`)

| Elle type | TOML type | Notes |
|-----------|-----------|-------|
| string | String | |
| int | Integer | |
| float | Float | |
| bool | Boolean | |
| array / @array | Array | Recursive |
| struct / @struct | Table | Keyword keys drop the `:` prefix |
| nil | — | Error: TOML has no null type |
| other | — | Error: unsupported type |

## Implementation

Uses the `toml = "0.8"` crate. `toml/parse` calls `toml::from_str::<toml::Value>()`,
then recursively converts `toml::Value` to Elle `Value`. `toml/encode` does the
reverse: recursively converts Elle `Value` to `toml::Value`, then serializes with
`toml::to_string()`.

Parsed structs are immutable (`Value::struct_from`). Parsed arrays are immutable
(`Value::array`). This follows the convention established by the TOML/YAML spec:
config data is read-only by nature.

## Building

```bash
cargo build --release -p elle-toml
# Output: target/release/libelle_toml.so
```

## Loading

```lisp
(def plugin (import-file "target/release/libelle_toml.so"))
(def parse-fn  (get plugin :parse))
(def encode-fn (get plugin :encode))
(parse-fn "[package]\nname = \"hello\"")
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
