# JSON Support

JSON serialization and deserialization primitives for Elle. Converts between Elle `Value` and JSON text.

## Primitives

| Primitive | Purpose |
|-----------|---------|
| `json/stringify` | Convert Elle value to JSON string |
| `json/parse` | Parse JSON string to Elle value |
| `json/pretty` | Convert Elle value to pretty-printed JSON |

## Type Mapping

| Elle Type | JSON Type |
|-----------|-----------|
| `nil` | `null` |
| `true`/`false` | `true`/`false` |
| Integer | Number |
| Float | Number |
| String | String |
| List/Array | Array |
| Table/Struct | Object |

## Examples

```lisp
(json/stringify {:name "Alice" :age 30})
;; => "{\"name\":\"Alice\",\"age\":30}"

(json/parse "{\"x\": 1, \"y\": 2}")
;; => {:x 1 :y 2}

(json/pretty {:items [1 2 3]})
;; => "{\n  \"items\": [\n    1,\n    2,\n    3\n  ]\n}"
```

## Error Handling

JSON primitives return errors for:

- **Invalid JSON**: Malformed input to `json/parse`
- **Unsupported types**: Values that can't be serialized (e.g., closures)
- **Circular references**: Tables that reference themselves

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/primitives/`](../) - other built-in functions
- [`src/value/`](../../value/) - runtime value representation
