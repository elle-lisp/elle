# Elle-Lint: Opinionated Linter for Elle Lisp

A comprehensive, opinionated static-analysis linter for Elle Lisp that enforces best practices and catches semantic errors before runtime.

## Features

### Implemented Rules

#### 1. Naming Conventions (`naming-kebab-case`) [WARNING]

Enforces kebab-case naming for all identifiers.

**Requirements:**
- User-defined functions and variables must use kebab-case (lowercase with hyphens)
- Single-letter variables allowed: `x`, `y`, `f`
- Suffix exceptions:
  - `?` allowed for predicates: `number?`, `empty?`
  - `!` allowed for mutations: `set!`, `vector-set!`

**Examples:**

```lisp
✗ (var myVariable 10)      ; Error: should be my-variable
✓ (var my-variable 10)     ; Correct

✗ (def isEmpty (fn [x] false))  ; Error: should be empty?
✓ (def empty? (fn [x] false))   ; Correct

✗ (def setValue! (fn [x v] v))  ; Error: should be set-value!
✓ (def set-value! (fn [x v] v)) ; Correct
```

### Planned Rules

- **Arity Validation**: Check function calls have correct number of arguments
- **Undefined Functions**: Detect calls to undefined functions
- **Undefined Variables**: Warn about unbound variable references
- **Unused Variables**: Identify unused bindings (with `_prefix` ignoring)
- **Pattern Matching**: Validate completeness and reachability
- **Module Boundaries**: Enforce module exports and prevent circular dependencies
- **Performance Hints**: Suggest vectors over lists for indexed access

## Installation

### Prerequisites
- Rust 1.70+ and Cargo

### Build from Source

```bash
cd elle/elle-lint
cargo build --release
```

The binary will be at `target/release/elle-lint`.

## Usage

### Basic

```bash
# Lint a single file
elle-lint script.l

# Lint a directory (recursive)
elle-lint src/

# Lint multiple files
elle-lint file1.l file2.l src/
```

### Output Format

```bash
# Human-readable (default)
elle-lint script.l --format text

# JSON for IDE integration
elle-lint script.l --format json
```

### Severity Filtering

```bash
# Show only errors (exit code 1 if any)
elle-lint script.l --level error

# Show errors and warnings (exit code 2 if warnings, 1 if errors)
elle-lint script.l --level warning

# Show all messages including info (default)
elle-lint script.l --level info
```

### Help

```bash
elle-lint --help
```

## Output Examples

### Human-Readable Format

```
script.l:5:2 warning: naming-kebab-case
  --> script.l:5
    |
  5 | (var myVariable 42)
    |  ^^^^^^^^^^^^

identifier 'myVariable' should use kebab-case

suggestions:
  - rename to 'my-variable'
```

### JSON Format

```json
{
  "diagnostics": [
    {
      "severity": "warning",
      "code": "W001",
      "rule": "naming-kebab-case",
      "message": "identifier 'myVariable' should use kebab-case",
      "file": "script.l",
      "line": 5,
      "column": 2,
      "context": "(var myVariable ...)",
      "suggestions": ["rename to 'my-variable'"]
    }
  ]
}
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | No issues found (or only INFO level messages) |
| 1 | One or more ERRORS found |
| 2 | WARNINGS found (no errors) |

## Design Philosophy

### Opinionated

Elle-lint enforces **one right way** to write Elle code. Rather than offering multiple style options, it makes opinionated choices based on industry best practices and Elle community conventions.

### Minimal Configuration

There is no `.elle-lint.toml` or complex configuration file. The only configuration is:
- `--format` (output format)
- `--level` (minimum severity to show)

Rules cannot be individually enabled/disabled.

### Static Analysis Only

Elle-lint performs pure static analysis without executing code. This means:
- ✅ Fast feedback (< 100ms for typical files)
- ✅ Works on incomplete code (during editing)
- ✅ No side effects or runtime errors
- ✅ Can be run in CI/CD pipelines

### Clear Error Messages

Each diagnostic includes:
- File, line, and column location
- Severity level (Error/Warning/Info)
- Rule code (e.g., W001)
- Clear message explaining the issue
- Code context with visual pointer
- Actionable suggestions

## Examples

### Example 1: Naming Convention Violation

```lisp
; bad-code.l
(def myFunction (fn [x] (* x x)))
(var result (myFunction 5))
```

```bash
$ elle-lint bad-code.l
bad-code.l:1:2 warning: naming-kebab-case
  --> bad-code.l:1
    |
  1 | (var myFunction ...)
    |  ^^^^^^^^^^^^

identifier 'myFunction' should use kebab-case

suggestions:
  - rename to 'my-function'
```

### Example 2: Multiple Files with JSON Output

```bash
$ elle-lint src/ --format json
{
  "diagnostics": [
    {
      "file": "src/math.l",
      "line": 3,
      "rule": "naming-kebab-case",
      ...
    }
  ]
}
```

## Integration with Tools

### VS Code

With JSON output, you can integrate elle-lint with VS Code:

```json
{
  "elle.lintCommand": "elle-lint",
  "elle.lintArgs": ["--format", "json"],
  "elle.lintOnSave": true
}
```

### Emacs

```elisp
(add-hook 'elle-mode-hook
  (lambda ()
    (setq flycheck-checker 'elle-lint)
    (flycheck-mode)))
```

### GitHub Actions

```yaml
- name: Lint Elle Code
  run: |
    cargo install --path elle/elle-lint
    elle-lint src/ --level error
```

## Rules Reference

### Severity Levels

| Level | Description | Exit Code |
|-------|-------------|-----------|
| **Error** | Code won't compile/run correctly | 1 |
| **Warning** | Code works but is suspicious/non-standard | 2 |
| **Info** | Suggestions for improvement (performance, style) | 0 |

### Error Codes

| Code | Rule | Severity |
|------|------|----------|
| E001 | undefined-function | Error |
| E002 | undefined-variable | Error |
| E003 | arity-mismatch | Error |
| W001 | naming-kebab-case | Warning |
| W002 | naming-builtin-shadowing | Warning |
| W003 | unused-variable | Warning |
| I001 | performance-hint | Info |

## Development

### Running Tests

```bash
cargo test
```

### Testing a File

```bash
cargo run --bin elle-lint -- tests/fixtures/naming-bad.l
```

### Adding New Rules

1. Create a new file in `src/rules/my_rule.rs`
2. Implement the check function
3. Register it in `src/rules/mod.rs`
4. Add tests in the rule module

### Code Style

- Use `snake_case` for Rust functions
- Use `kebab-case` for Elle identifiers (as linted!)
- Follow Rust idioms and use `cargo fmt`

## Roadmap

### Phase 1: Complete (Current)
- ✓ Naming convention validation
- ✓ CLI framework with multiple output formats
- ✓ Basic testing infrastructure

### Phase 2: Soon
- [ ] Arity validation and undefined function detection
- [ ] Unused variable detection
- [ ] Pattern matching validation
- [ ] Module boundary rules
- [ ] Performance hints

### Phase 3: Future
- [ ] Custom rule definitions
- [ ] IDE integration plugins (VS Code, Emacs, Vim)
- [ ] Auto-fixer (`--fix` flag)
- [ ] Configuration file support

## License

Same as Elle interpreter

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/your-feature`)
3. Add tests for new rules
4. Ensure all tests pass (`cargo test`)
5. Format code (`cargo fmt`)
6. Check for warnings (`cargo clippy`)
7. Submit a pull request

## See Also

- [Elle Interpreter](../)
- [Elle Examples](../examples/)
- [Elle Documentation](../ROADMAP.md)
