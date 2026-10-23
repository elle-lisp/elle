# The elle crate

<!-- audited: 2026-10-22 -->

How source text becomes bytecode that a VM runs, the module that owns each stage, and the invariants every change keeps.

The implementation targets correctness, performance and clarity, in that
order. It compiles through two intermediate representations, HIR and LIR. It
resolves every binding lexically with closure capture analysis, and carries
one system of signals and capabilities: signals flow up from callee to
caller, and capabilities flow down from parent fiber to child
([signals](../docs/signals/index.md)).

## The pipeline

```text
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

This is the only compilation pipeline. Source locations flow through all of
it: a Syntax span becomes a HIR span, then the span on each LIR node, then the
`LocationMap` in the bytecode, so an error message names `file:line:col`. The
JIT, the WASM tier and the MLIR tier compile from the LIR
([pipeline](../docs/pipeline.md)).

## Modules

| Module | Owns |
|--------|------|
| `reader` | Lexing and parsing to `Syntax` |
| `syntax` | Syntax types and macro expansion |
| `hir` | Binding resolution, capture analysis, signal inference, linting, symbol and docstring extraction |
| `lir` | SSA form with virtual registers and basic blocks, built in a working region and frozen into plain records, each node carrying its span |
| `compiler` | Bytecode instruction definitions and debug formatting |
| `pipeline` | The compilation entry points |
| `vm` | Bytecode execution, and the builtin documentation store |
| `value` | The 16-byte tagged-union `Value` ([values](../docs/impl/values.md)) |
| `primitives` | Built-in functions; `(help)` lists them by category |
| `arithmetic` | The arithmetic the VM and the primitives share |
| `signals` | The `Signal` type, the signal registry, and `CAP_MASK` |
| `io`, `port` | I/O requests, their backends and timeouts; the port, a file descriptor with direction, encoding and kind |
| `error` | `LocationMap`, from bytecode offset to source location |
| `runtime` | The per-instance `Runtime` and `RuntimeCore`, which own the VM, the symbol table, the compile context and the heap |
| `program` | The run path `elle` and `elle-rig` share: a file, `-e`, stdin or the REPL |
| `symbol`, `namehash` | Symbol identity, the name's hash, and the per-instance display memo ([symbol](../docs/impl/symbol.md)) |
| `config` | The global configuration set at startup, and the per-VM configuration `vm/config` reads |
| `image` | Dumping a sealed value graph as page bytes and relocations, and mapping it back ([image](../docs/impl/image.md)) |
| `jit` | Compilation through Cranelift; it refuses `MakeClosure` and keyword collectors, whatever the signal |
| `wasm` | The WebAssembly backend under Wasmtime, in a `wasm` build ([wasm](../docs/impl/wasm.md)) |
| `mlir` | The MLIR backend |
| `ffi` | C interop through libloading and libffi |
| `plugin`, `plugin_api` | Loading a Rust cdylib plugin, and the stable ABI it calls ([plugins](../docs/plugins.md)) |
| `lint`, `symbols`, `lsp` | Diagnostics, the symbol index for IDE features, and the language server |
| `formatter`, `rewrite`, `epoch` | `elle fmt`, token-level source rewriting, and the epoch migrations the compiler applies before expansion |
| `segment` | Unicode grapheme segmentation: vendored table generations, chosen per VM |
| `path` | UTF-8 path operations |
| `repl` | The read-eval-print loop |
| `test`, `semver` | The `elle test` runner and `elle semver`, both written in Elle |
| `dump`, `trace` | The `--dump` artifacts, and the `--trace` channels |

[stdlib.lisp](stdlib.lisp) is the standard library every instance loads at
startup ([stdlib](../docs/stdlib.md)).

## Invariants

1. **Bindings are resolved at analysis time.** HIR holds a `Binding`, a `u32`
   index into the `BindingArena` the pipeline owns, never a symbol. A symbol
   lookup at run time is a defect.
2. **A closure captures by value into its environment.** An immutable
   captured local is captured directly. A mutable captured local and a mutated
   parameter go through a `CaptureCell`, and the template's
   `capture_params_mask` names the parameters to wrap.
3. **Signals are inferred, and capabilities are enforced at run time.** The
   analyzer infers each function's signal from its callees. A ceiling such as
   `(silent!)` checks the inference and never replaces it. A parent fiber
   withholds a capability with `:deny`, and a denied operation becomes a
   signal the parent can catch ([capabilities](../docs/signals/capabilities.md)).
4. **The VM is stack-based for operands, and register-addressed for locals.**
   An instruction names a local by its index, and pushes its result to the
   operand stack.
5. **Errors propagate.** A function returns `LResult<T>`. A caught error is
   handled or propagated, never dropped.

## Where to start

1. Read [pipeline/mod.rs](pipeline/mod.rs). It names each stage and the entry
   points over them.
2. Read a test under [tests/lang](../tests/lang/AGENTS.md) to see the surface
   syntax at work.
3. Read [value/mod.rs](value/mod.rs) for the runtime representation.
4. Read a failing test to learn what is expected.
5. Read the index of the module you change, and the documents it lists.
6. Read [the cookbook](../docs/cookbook/index.md) for the steps of a change
   that crosses modules.
