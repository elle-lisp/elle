# JSON Support

JSON serialization and deserialization primitives for Elle. Converts between Elle `Value` and JSON text.

## Primitives

| Primitive | Purpose |
|-----------|---------|
| `json/serialize` | Convert Elle value to JSON string |
| `json/parse` | Parse JSON string to Elle value |
| `json/pretty` | Convert Elle value to pretty-printed JSON |

## Type Mapping

Serializing, Elle to JSON:

| Elle Type | JSON Type |
|-----------|-----------|
| `nil` | `null` |
| `true`/`false` | `true`/`false` |
| Integer | Number, written without a decimal point |
| Float | Number, written with a decimal point |
| String | String |
| List/Array | Array |
| Table/Struct | Object |

Parsing, JSON to Elle:

| JSON Type | Elle Type |
|-----------|-----------|
| `null` | `nil` |
| `true`/`false` | `true`/`false` |
| Number without a decimal point | Integer |
| Number with a decimal point | Float |
| String | String |
| Array | List |
| Object | `@struct`, keyed by string |

Two properties of a parsed object catch callers out.

**Keys are strings, not keywords.** `(get parsed "id")` reads a field, and
`(get parsed :id)` returns `nil` rather than signalling, so the mistake is
silent. Pass `:keys :keyword` to opt in to keyword keys.

**The result is mutable.** `json/parse` returns an `@struct`, so `put` and `del`
change it in place instead of returning a new value. Copy with `freeze` first
when the original has to survive the change.

## Examples

```lisp
(json/serialize {:name "Alice" :age 30})
# => "{\"age\":30,\"name\":\"Alice\"}"

(json/parse "{\"x\": 1, \"y\": 2}")
# => @{"x" 1 "y" 2}

(json/parse "{\"x\": 1}" :keys :keyword)
# => @{:x 1}

(json/pretty {:items [1 2 3]})
# => "{\n  \"items\": [\n    1,\n    2,\n    3\n  ]\n}"
```

`json/serialize` writes object keys in sorted order, at every depth and inside
arrays, so the same value always produces the same bytes. Array element order is
preserved, as JSON requires.

## Error Handling

The JSON primitives signal an error for:

- **Invalid JSON**: Malformed input to `json/parse`
- **Unsupported types**: Values that can't be serialized (e.g., closures)
- **Circular references**: Tables that reference themselves

Catch one with `try` or `protect`, as with any other error.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/primitives/`](../) - other built-in functions
- [`src/value/`](../../value/) - runtime value representation
