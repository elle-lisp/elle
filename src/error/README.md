# Error Handling

Elle uses a single unified error type, `LError`, throughout the Rust
implementation. This provides structured errors with source locations,
stack traces, and consistent formatting.

## Philosophy

Errors are not strings. They're typed data that can be:
- Inspected programmatically
- Enriched with location information as they propagate
- Formatted consistently for display
- Distinguished by kind for different handling

We never silently drop errors. Functions return `LResult<T>`, and callers
either handle errors meaningfully or propagate them with `?`.

## Quick Start

```rust
use crate::error::{LError, LResult};

fn divide(a: i64, b: i64) -> LResult<i64> {
    if b == 0 {
        return Err(LError::division_by_zero());
    }
    Ok(a / b)
}

fn get_int(value: &Value) -> LResult<i64> {
    match value {
        Value::Int(n) => Ok(*n),
        _ => Err(LError::type_mismatch("integer", value.type_name())),
    }
}
```

## Adding Location Information

Errors can carry source locations for better diagnostics:

```rust
let result = parse_expr(tokens)
    .map_err(|e| e.with_location(current_loc))?;
```

## Error Kinds

Rather than string messages, errors are categorized:

- **Type errors**: `type_mismatch`, `undefined_variable`
- **Arity errors**: `arity_mismatch`, `arity_at_least`, `arity_range`
- **Arithmetic**: `division_by_zero`, `numeric_overflow`
- **FFI**: `library_not_found`, `symbol_not_found`, `ffi_error`
- **Compilation**: `syntax_error`, `compile_error`, `macro_error`
- **IO**: `file_not_found`, `file_read_error`

See `builders.rs` for the full list of constructors.

## Stack Traces

For runtime errors, traces can be attached:

```rust
LError::runtime_error("something went wrong")
    .with_trace(TraceSource::Vm(captured_frames))
```

The trace renders as part of the error display, showing the call stack
at the point of failure.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/value/condition.rs` - Elle-level condition system (user-facing exceptions)
