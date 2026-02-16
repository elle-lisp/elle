# Formatter

The formatter pretty-prints Elle source code with consistent style.

## Usage

```rust
use elle::formatter::{format_code, FormatterConfig};

let source = "(if(> x 0)(+ x 1)(- x 1))";
let config = FormatterConfig::default();
let formatted = format_code(source, config)?;
// Result:
// (if (> x 0)
//     (+ x 1)
//     (- x 1))
```

## Configuration

`FormatterConfig` controls formatting behavior:

```rust
pub struct FormatterConfig {
    pub indent_width: usize,     // Spaces per indent level
    pub max_line_length: usize,  // Wrap threshold
    // ...
}
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `elle-lint/` - uses formatter for style checking
