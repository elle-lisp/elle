# JSON Support

JSON parsing and serialization for Elle. Converts between an Elle `Value`
and JSON text. The parser and the serializer are hand-written; the module
depends on no external JSON library.

## Primitives

| Primitive | Purpose |
|-----------|---------|
| `json/parse` | Parse a JSON string into an immutable Elle value |
| `json/serialize` | Convert an Elle value to a compact JSON string |
| `json/pretty` | Convert an Elle value to a pretty-printed JSON string |

## Type mapping

The mapping is not symmetric, so each direction has its own table.

`json/parse` produces these Elle types:

| JSON | Elle |
|------|------|
| `null` | `nil` |
| `true` / `false` | `true` / `false` |
| Number without `.`, `e`, or `E` | Integer |
| Number with `.`, `e`, or `E` | Float |
| String | String |
| Array | Immutable array (`[...]`) |
| Object | Immutable struct (`{...}`) |

`json/serialize` and `json/pretty` accept these Elle types:

| Elle | JSON |
|------|------|
| `nil` | `null` |
| `true` / `false` | `true` / `false` |
| Integer | Number |
| Float (finite) | Number |
| String | String |
| Keyword | String |
| List, array, `@array` | Array |
| Set, `@set` | Array |
| Struct, `@struct` | Object |

## Parsed values are immutable

`json/parse` returns an immutable value at every depth. Arrays, structs,
and every value nested inside them are immutable. `put` and `del` on the
result return a new value and leave the parsed value unchanged, so a
caller can compare a field after removing it, and two callers can share
one parsed document without copying it.

```lisp
(def doc (json/parse "{\"a\": [1, 2], \"b\": 3}"))

(immutable? doc)                 # => true
(immutable? (get doc "a"))       # => true

(put doc "b" 99)                 # => {"a" [1 2] "b" 99}
doc                              # => {"a" [1 2] "b" 3}
```

To get a mutable copy, use `thaw`. `thaw` is shallow: it converts the
value you hand it, not the values nested inside it.

```lisp
(def m (thaw (json/parse "{\"a\": 1}")))

(mutable? m)                     # => true
(put m "b" 2)                    # => @{"a" 1 "b" 2}
```

## Object keys

`json/parse` uses string keys by default. Pass `:keys :keyword` to get
keyword keys instead. The option applies to nested objects too.

```lisp
(get (json/parse "{\"x\": 1}") "x")               # => 1
(get (json/parse "{\"x\": 1}") :x)                # => nil
(get (json/parse "{\"x\": 1}" :keys :keyword) :x) # => 1
```

The default is the expensive case to get wrong, because it fails
silently: `get` with the wrong key kind returns `nil` and signals
nothing.

## Sorted output

`json/serialize` and `json/pretty` write object keys in sorted order at
every depth, whatever order the source struct was built in. Callers who
hash serialized output depend on this property.

```lisp
(json/serialize {:name "Alice" :age 30})
# => "{\"age\":30,\"name\":\"Alice\"}"

(println (json/pretty {:items [1 2 3]}))
# {
#   "items": [
#     1,
#     2,
#     3
#   ]
# }
```

## Errors

The JSON primitives signal an error for:

- **Malformed JSON** — bad tokens, unterminated strings, trailing
  content, trailing commas, leading zeros, and lone surrogates.
- **Unserializable values** — closures, symbols, fibers, ports, and the
  other types with no JSON counterpart.
- **Non-finite floats** — `json/serialize` rejects NaN and infinity,
  which JSON cannot represent.

## See also

- [AGENTS.md](AGENTS.md) — technical reference for LLM agents
- [`tests/elle/prim-json.lisp`](../../../tests/elle/prim-json.lisp) — the
  behavior tests for these primitives
- [`src/primitives/`](../) — other built-in functions
- [`src/value/`](../../value/) — runtime value representation
