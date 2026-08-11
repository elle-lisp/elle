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
| `JsonParser::new(input, ctx)` | Create parser for JSON string (string keys); allocates through `ctx` |
| `JsonParser::new_with_opts(input, use_keyword_keys, ctx)` | Create parser with options; allocates through `ctx` |
| `JsonParser::parse()` | Parse JSON → Elle value |
| `serialize_value(value)` | Serialize value → JSON string |
| `serialize_value_pretty(value, indent)` | Serialize value → pretty JSON string |
| `escape_json_string(s)` | Escape string for JSON output |

## Primitives

| Name | Arity | Signal | Purpose |
|------|-------|--------|---------|
| `json/parse` | 1–3 | Errors | Parse JSON string to Elle value; accepts `:keys :keyword` option |
| `json/serialize` | 1 | Errors | Serialize Elle value to compact JSON |
| `json/pretty` | 1 | Errors | Serialize Elle value to pretty JSON |

### json/parse options

`(json/parse json-string)` — parse with default string keys for JSON object fields.

`(json/parse json-string :keys :keyword)` — parse JSON objects using keyword keys (`:field`) instead of string keys (`"field"`). The option applies recursively to nested objects. Arrays are unaffected.

### Error kinds

| Condition | Kind |
|-----------|------|
| Malformed input to `json/parse` | `serde-error` |
| A value neither serializer can encode, such as a closure | `serde-error` |
| A non-string first argument to `json/parse` | `type-error` |
| 2 args to `json/parse` | `arity-error` |
| 3 args to `json/parse` with an unrecognized key name or value | `argument-error` |

Both directions of the codec report `serde-error`, so one `catch` covers the
whole module.

## JSON → Elle value mapping (`json/parse`)

| JSON | Elle | Constructor |
|------|------|-------------|
| `null` | `nil` | `Value::NIL` |
| `true` / `false` | `true` / `false` | `Value::bool()` |
| Number (int) | Integer | `Value::int()` |
| Number (float) | Float | `Value::float()` |
| String | String | `ctx.string()` |
| Array | Immutable array (`[...]`) | `ctx.array()` |
| Object | Immutable struct (`{...}`) | `ctx.struct_from()` |

## Elle → JSON value mapping (`json/serialize`, `json/pretty`)

The serializer accepts more types than the parser produces. Lists,
`@arrays`, and immutable arrays all write as JSON arrays; sets and
`@sets` write as JSON arrays; `@structs` and immutable structs both
write as JSON objects; keywords write as JSON strings.

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
- Collections (@arrays, @structs, arrays, structs)
- Nested structures (recursive)
- Pretty-printing with configurable indentation

## Invariants

1. **JSON null maps to Elle nil.** `Value::NIL` serializes to `null` and `null` parses to `Value::NIL`.

2. **A parsed value is immutable at every depth.** `JsonParser` builds only
   immutable arrays and immutable structs, so no part of a parsed document can
   be changed in place. Callers share one parsed document without copying it,
   and `put`/`del` on any part of it return a new value.

3. **JSON arrays map to immutable Elle arrays.** `ctx.array()`, not the
   mutable `ctx.array_mut()` and not a cons list.

4. **JSON objects map to immutable Elle structs.** `ctx.struct_from()`, not
   the mutable `ctx.struct_mut_from()`.

5. **String escaping is bidirectional.** `serialize_value()` escapes special characters; `JsonParser` unescapes them.

6. **No external JSON library.** All parsing and serialization is hand-written to avoid dependencies.

6. **All three primitives declare `Signal::errors()`.** The declaration matches the `SIG_ERROR` each returns, so effect inference propagates `:error` to callers and `try` reaches the failure at any call depth. `tests/elle/prim-json.lisp` pins this.

## Dependents

- `primitives/registration.rs` — registers JSON primitives
- `primitives/module_init.rs` — initializes JSON module
- Elle code — via `json/parse`, `json/serialize`, `json/pretty`
