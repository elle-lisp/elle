# Compilation Pipeline

The pipeline module provides high-level entry points for the full compilation process, from source code to bytecode execution.

## Pipeline Stages

```
Source → Reader → Syntax → Expander → Analyzer → Lowerer → Emitter → Bytecode → VM
```

## Entry Points

| Function | Purpose |
|----------|---------|
| `compile(source)` | Parse, expand, analyze, lower, emit bytecode |
| `analyze(source)` | Parse, expand, analyze (stop before lowering) |
| `eval(source)` | Compile and execute in the VM |
| `eval_syntax(syntax)` | Expand, analyze, lower, emit, execute |

## Error Handling

All pipeline functions return `LResult<T>` which propagates errors with source location information:

```rust
match compile(source) {
    Ok(bytecode) => { /* use bytecode */ },
    Err(e) => eprintln!("Error: {}", e),  // Includes file:line:col
}
```

## Usage Examples

```rust
// Compile to bytecode
let bytecode = compile("(+ 1 2)")?;

// Analyze without lowering
let hir = analyze("(let ((x 10)) (+ x 1))")?;

// Compile and execute
let result = eval("(+ 1 2)")?;
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/reader/`](../reader/) - parsing
- [`src/syntax/`](../syntax/) - macro expansion
- [`src/hir/`](../hir/) - analysis
- [`src/lir/`](../lir/) - lowering
- [`src/compiler/`](../compiler/) - bytecode emission
- [`src/vm/`](../vm/) - execution
