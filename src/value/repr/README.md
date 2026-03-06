# Value Representation

The value representation module implements NaN-boxing, the technique used to represent all Elle runtime values in 8 bytes.

## NaN-Boxing

NaN-boxing uses the IEEE 754 floating-point format to encode multiple types in a single 64-bit value:

- **Floating-point numbers**: Use the standard IEEE 754 representation
- **Integers**: Encoded in the mantissa with a special exponent
- **Pointers**: Heap-allocated objects encoded as tagged pointers
- **Immediates**: Booleans, nil, keywords encoded as special bit patterns

## Value Types

| Type | Representation | Size |
|------|----------------|------|
| Integer | Mantissa + exponent | 8 bytes |
| Float | IEEE 754 | 8 bytes |
| String | Pointer to heap | 8 bytes |
| List | Pointer to heap | 8 bytes |
| Array | Pointer to heap | 8 bytes |
| Table | Pointer to heap | 8 bytes |
| Closure | Pointer to heap | 8 bytes |
| Boolean | Immediate | 8 bytes |
| Nil | Immediate | 8 bytes |
| Keyword | Immediate | 8 bytes |

## Key Modules

| Module | Purpose |
|--------|---------|
| [`constructors.rs`](constructors.rs) | Create values: `Value::int()`, `Value::string()`, etc. |
| [`accessors.rs`](accessors.rs) | Extract values: `as_int()`, `as_string()`, etc. |
| [`traits.rs`](traits.rs) | Implement `PartialEq`, `Hash`, `Display` |

## Performance

NaN-boxing provides:

- **Compact representation**: All values fit in 8 bytes
- **Fast operations**: No indirection for immediates
- **Efficient GC**: Pointer tagging enables fast type checks

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/value/`](../) - value types and heap objects
- [`src/value/heap.rs`](../heap.rs) - heap-allocated object types
