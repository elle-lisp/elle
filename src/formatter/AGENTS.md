# formatter

Code formatting for Elle source. Pretty-prints code with consistent style.

## Responsibility

- Format Elle source code
- Apply configurable style rules
- Preserve semantics while improving readability

Does NOT:
- Parse code (uses `reader`)
- Validate code (just formats)
- Modify file system (caller handles I/O)

## Interface

| Type | Purpose |
|------|---------|
| `FormatterConfig` | Style configuration |
| `format_code(src, config)` | Format source string |

## Usage

```rust
use elle::formatter::{format_code, FormatterConfig};

let config = FormatterConfig::default();
let formatted = format_code(source, config)?;
```

## Dependents

- `elle-lint` - formatting checks
- CLI - `elle fmt` command (if implemented)

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 20 | Re-exports |
| `config.rs` | ~50 | `FormatterConfig` |
| `core.rs` | ~200 | Formatting logic |
