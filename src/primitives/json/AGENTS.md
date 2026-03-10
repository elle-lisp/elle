# primitives/json

JSON parsing and serialization primitives.

## Responsibility

- Parse JSON strings into Elle values
- Serialize Elle values to JSON (compact and pretty-printed)
- Provide hand-written recursive descent parser (no external JSON libraries)

## Submodules

| Module | Purpose |
|--------|---------|
| `parser.rs` | Recursive descent JSON parser |
| `serializer.rs` | JSON serialization (compact and pretty-printed) |

## Interface

| Function | Purpose |
|----------|---------|
| `prim_json_parse(args)` | Parse JSON string → Elle value |
| `prim_json_serialize(args)` | Serialize Elle value → compact JSON string |
| `prim_json_serialize_pretty(args)` | Serialize Elle value → pretty-printed JSON string |
| `JsonParser::new(input)` | Create parser for JSON string |
| `JsonParser::parse()` | Parse JSON → Elle value |
| `serialize_value(value)` | Serialize value → JSON string |
| `serialize_value_pretty(value, indent)` | Serialize value → pretty JSON string |
| `escape_json_string(s)` | Escape string for JSON output |

## Primitives

| Name | Arity | Effect | Purpose |
|------|-------|--------|---------|
| `json/parse` | 1 | Inert | Parse JSON string to Elle value |
| `json/serialize` | 1 | Inert | Serialize Elle value to compact JSON |
| `json/serialize-pretty` | 1 | Inert | Serialize Elle value to pretty JSON |

## JSON ↔ Elle value mapping

| JSON | Elle |
|------|------|
| `null` | `nil` |
| `true` / `false` | `true` / `false` |
| Number (int) | `Value::int()` |
| Number (float) | `Value::float()` |
| String | `Value::string()` |
| Array | `Value::array()` (mutable) |
| Object | `Value::table()` (mutable) |

## Parser implementation

`JsonParser` is a hand-written recursive descent parser with:
- Whitespace skipping
- Number parsing (integers and floats)
- String parsing with escape sequence handling
- Array parsing (recursive)
- Object parsing (recursive, keys must be strings)
- Error reporting with position information

## Serializer implementation

`serialize_value()` and `serialize_value_pretty()` handle:
- Immediate values (nil, bool, int, float)
- Strings (with escape sequences)
- Collections (arrays, tables, tuples, structs)
- Nested structures (recursive)
- Pretty-printing with configurable indentation

## Invariants

1. **JSON null maps to Elle nil.** `Value::NIL` serializes to `null` and `null` parses to `Value::NIL`.

2. **JSON arrays map to Elle arrays.** Arrays are mutable (`Value::array()`), not immutable tuples.

3. **JSON objects map to Elle tables.** Objects are mutable (`Value::table()`), not immutable structs.

4. **String escaping is bidirectional.** `serialize_value()` escapes special characters; `JsonParser` unescapes them.

5. **No external JSON library.** All parsing and serialization is hand-written to avoid dependencies.

## Dependents

- `primitives/registration.rs` — registers JSON primitives
- `primitives/module_init.rs` — initializes JSON module
- Elle code — via `json/parse`, `json/serialize`, `json/serialize-pretty`

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 601 | Primitive definitions and entry points |
| `parser.rs` | ~400 | Recursive descent JSON parser |
| `serializer.rs` | ~300 | JSON serialization (compact and pretty) |
