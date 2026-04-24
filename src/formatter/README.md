# Formatter

Opinionated pretty-printer for Elle source. One canonical style. Idempotent.

## CLI

```
elle fmt [OPTIONS] <file...>     Format files in place
elle fmt < file.lisp             Format stdin to stdout
elle fmt --check lib/*.lisp      Check without writing (exit 1 if changes needed)
```

Options:
- `--check` — report files that need formatting, enforce column limits
- `--line-length=N` — target line width (default: 80)
- `--indent-width=N` — spaces per indent level (default: 2)

## API

```rust
use elle::formatter::{format_code, FormatterConfig};

let config = FormatterConfig::default();
let formatted = format_code(source, &config)?;
```

## Design

Wadler-style document algebra with column-aware `Align`. The renderer
tracks indent as absolute columns so forms nested inline (fn inside
letrec bindings, and inside when, structs inside error calls) align
relative to their actual position, not their Nest level.

See [AGENTS.md](AGENTS.md) for the full Doc algebra, dispatch table,
and invariants.
