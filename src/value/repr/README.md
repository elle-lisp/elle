# Value Representation

The value representation module implements the tagged union used to represent all Elle runtime values in 16 bytes.

## Tagged Union

Each `Value` is a `(tag: u64, payload: u64)` pair. The tag discriminates the type, and the payload carries the data:

- **Floating-point numbers**: Payload is the IEEE 754 bit pattern
- **Integers**: Payload is a full-range i64
- **Pointers**: Payload is a pointer to a heap-allocated object
- **Immediates**: Booleans, nil, keywords encoded in the payload

## Value Types

| Type | Representation | Size |
|------|----------------|------|
| Integer | Tag + i64 payload | 16 bytes |
| Float | Tag + IEEE 754 payload | 16 bytes |
| String | Tag + pointer to heap | 16 bytes |
| List | Tag + pointer to heap | 16 bytes |
| Array | Tag + pointer to heap | 16 bytes |
| Table | Tag + pointer to heap | 16 bytes |
| Closure | Tag + pointer to heap | 16 bytes |
| Boolean | Tag + immediate | 16 bytes |
| Nil | Tag + immediate | 16 bytes |
| Keyword | Tag + immediate | 16 bytes |

## Key Modules

| Module | Purpose |
|--------|---------|
| [`constructors.rs`](constructors.rs) | Create values: `Value::int()`, `Value::string()`, etc. |
| [`accessors.rs`](accessors.rs) | Extract values: `as_int()`, `as_string()`, etc. |
| [`traits.rs`](traits.rs) | Implement `PartialEq`, `Hash`, `Display` |

## Performance

The tagged-union representation provides:

- **Fast operations**: No indirection for immediates
- **Efficient type checks**: Tag comparison is a single integer compare
- **Full-range integers**: i64 without truncation

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/value/`](../) - value types and heap objects
- [`src/value/heap.rs`](../heap.rs) - heap-allocated object types
