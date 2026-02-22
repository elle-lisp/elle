# error

Unified error system. All failures flow through `LError`.

## Responsibility

Single error type for the entire crate. Provides:
- Categorized error kinds (not stringly-typed)
- Source locations
- Stack traces (VM or CPS)
- Human-readable formatting

Does NOT handle Elle-level errors (error tuples `[:keyword "message"]`).
Those are user-facing; this module is for Rust-level errors.

## Interface

| Type | Purpose |
|------|---------|
| `LError` | The error type. Has `kind`, `location`, `trace`. |
| `ErrorKind` | Enum of all error categories |
| `LResult<T>` | `Result<T, LError>` |
| `SourceLoc` | Line/column position |
| `StackFrame` | Function name + location for traces |
| `TraceSource` | `None`, `Vm(frames)`, or `Cps(frames)` |

## Usage pattern

```rust
// Construction via builders (preferred)
LError::type_mismatch("integer", value.type_name())
LError::arity_mismatch(2, args.len())
LError::undefined_variable(&name)

// With location
LError::type_mismatch(...).with_location(loc)

// With trace
LError::runtime_error(...).with_trace(TraceSource::Vm(frames))
```

## Dependents

Everything. 34 files import from this module. Key consumers:
- `primitives/*` - all return `LResult<Value>`
- `value.rs` - accessor methods return `LResult`
- `vm/` - execution errors with traces
- `compiler/` - uses `LocationMap` for bytecode→source mapping

## Invariants

1. **Errors propagate, never silently drop.** Functions return `LResult`.
   If you handle an error, handle it meaningfully or re-raise.

2. **`ErrorKind` is exhaustive.** Add new variants for new error categories.
   Don't use `Generic` for things that deserve their own kind.

3. **`RuntimeError` is legacy.** New code should use `LError`. `RuntimeError`
   exists for compatibility during migration.

4. **Builders exist for all common cases.** Don't construct `LError::new()`
   directly; use `LError::type_mismatch()` etc. from `builders.rs`.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 320 | Re-exports + comprehensive tests |
| `types.rs` | 300 | `LError`, `ErrorKind`, `LResult`, Display impl |
| `builders.rs` | 160 | Constructor methods on `LError` |
| `runtime.rs` | 53 | Legacy `RuntimeError` (deprecate eventually) |
| `sourceloc.rs` | ~50 | `SourceLoc` definition |
| `formatting.rs` | ~100 | Rich error formatting |

## Anti-patterns

- `LError::generic("...")` when a specific `ErrorKind` exists
- `.unwrap()` instead of propagating with `?`
- Stringifying errors early (`err.to_string()`) - loses structure
- Catching errors just to re-wrap them in `Generic`
