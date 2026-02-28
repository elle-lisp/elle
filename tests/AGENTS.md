# Tests

## Directory structure

```
tests/
├── lib.rs              # Test harness — discovers all test modules
├── common/mod.rs       # Shared helpers (eval_source, setup)
├── fixtures/           # Static test data files (e.g., .lisp files for linter tests)
├── property/           # Property-based tests (proptest)
│   ├── mod.rs          # Module declarations
│   ├── strategies.rs   # Shared proptest strategies for generating Values and types
│   └── *.rs            # One file per domain (arithmetic, nanboxing, effects, etc.)
├── integration/        # Full-pipeline integration tests
│   ├── mod.rs          # Module declarations
│   └── *.rs            # One file per feature area
├── unittests/          # Unit tests for individual modules
│   ├── mod.rs          # Module declarations
│   └── *.rs            # One file per module under test
└── vm/                 # VM-specific tests (scope management, runtime behavior)
    ├── mod.rs          # Module declarations
    └── scope_test.rs   # ScopeStack and RuntimeScope tests
```

In addition to the `tests/` directory:

- **`src/` inline tests**: 58 modules in `src/` contain `#[cfg(test)]` modules
  with unit tests colocated next to the code they test. These cover reader,
  syntax, HIR, LIR, emitter, VM, value representation, effects, FFI, LSP,
  lint, JIT, formatter, pipeline, and more.

- **`examples/`**: Every `.lisp` file in `examples/` is an executable test.
  `cargo test --test '*'` runs them. They serve as both documentation and
  regression tests for the surface language.

## When to use each category

### Property tests (`tests/property/`)

Use for **invariants that must hold across all inputs**. These use proptest to
generate random inputs and verify properties like:

- Roundtrip fidelity (NaN-boxing, reader parse/display, type conversions)
- Mathematical laws (commutativity, associativity, identity, inverse)
- Type discrimination (exactly one type predicate is true for any Value)
- Determinism (same source always produces same result)
- Effect inference soundness (pure expressions never inferred as yielding)
- Bug regression across input ranges (not just the single case that triggered it)

Property tests answer: "Does this invariant hold for *all* valid inputs?"

### Integration tests (`tests/integration/`)

Use for **end-to-end pipeline behavior**. These evaluate Elle source code
through the full pipeline (Reader → Expander → Analyzer → Lowerer → Emitter →
VM) and check the result. They cover:

- Core language features (arithmetic, conditionals, lists, functions)
- Advanced features (closures, recursion, higher-order functions, match)
- Concurrency (fibers, coroutines, thread transfer)
- Effect enforcement (interprocedural effect tracking)
- Error reporting (error messages include correct source locations)
- Destructuring, blocks, splice, booleans, dispatch
- Prelude macros (defn, let*, when, unless, etc.)
- Lint and LSP features
- FFI integration
- JIT compilation
- REPL exit codes

Integration tests answer: "Does this Elle program produce the expected result?"

### Unit tests (`tests/unittests/`)

Use for **module internals that can be tested in isolation**. These test Rust
APIs directly without going through the full compilation pipeline:

- `value.rs` — Value construction, equality, truthiness, type conversions,
  list/array/cons operations, arity matching
- `symbol.rs` — Symbol interning, lookup, persistence, ordering
- `primitives.rs` — Primitive functions called directly via `call_primitive()`
- `closures_and_lambdas.rs` — Closure construction, capture, arity, effects
- `bytecode_debug.rs` — Bytecode compilation and debug output
- `hir_debug.rs` — HIR structure after analysis
- `lir_debug.rs` — LIR structure after lowering
- `jit.rs` — JIT compilation pipeline, hot function detection

Unit tests answer: "Does this Rust API behave correctly?"

### VM tests (`tests/vm/`)

Use for **VM runtime internals** that are below the integration level but above
individual module unit tests:

- `scope_test.rs` — ScopeStack push/pop, variable define/lookup/set,
  shadowing, isolation, scope types

VM tests answer: "Does the VM's internal machinery work correctly?"

### Inline tests (`src/**/mod.rs` with `#[cfg(test)]`)

Use for **tests tightly coupled to implementation details**. These live next to
the code they test and have access to private items. 58 modules have inline
tests covering: lexer, parser, syntax conversion, expander, analyzer, lowerer,
emitter, VM core, VM arithmetic, scope management, value representation,
closures, fibers, effects, FFI (marshal, callback, loader, types, call),
primitives (fibers, coroutines, FFI, process, JSON), JIT (compiler, dispatch,
group, runtime), LSP (completion, definition, hover, references, rename,
formatting, state), lint (rules, diagnostics, CLI), formatter, pipeline,
symbols, symbol table, error formatting, REPL, and arithmetic dispatch.

Inline tests answer: "Does this private implementation detail work correctly?"

## Test helpers

### `common/mod.rs`

Two functions, both initialize a full VM with primitives and stdlib:

**`eval_source(input: &str) -> Result<Value, String>`** — The canonical test
eval. Evaluates Elle source through the full pipeline. Handles multi-form
input via `eval_all`. Sets and clears thread-local VM/symbol-table context.
Use this for any test that needs to run Elle code.

```rust
use crate::common::eval_source;
let result = eval_source("(+ 1 2)").unwrap();
assert_eq!(result, Value::int(3));
```

**`setup() -> (SymbolTable, VM)`** — Returns an initialized (SymbolTable, VM)
pair with primitives and stdlib registered. Sets the symbol table context but
does NOT set VM context. Use this when you need direct access to the VM or
symbol table (e.g., calling `analyze()` or `compile()` directly, or looking up
primitives by name).

```rust
use crate::common::setup;
let (mut symbols, mut vm) = setup();
let result = analyze("(+ 1 2)", &mut symbols, &mut vm).unwrap();
```

Note: Some test files define their own local `setup()` that returns `(VM,
SymbolTable)` (reversed order) or omits stdlib. Check the file you're working
in.

### `property/strategies.rs`

8 public strategies for generating Elle values and FFI types:

| Strategy | Generates | Use for |
|----------|-----------|---------|
| `arb_immediate()` | nil, empty_list, true, false, ints, floats, symbols | Tests that don't need heap values |
| `arb_value()` | Everything from `arb_immediate()` + strings, cons, arrays (depth 3) | General value testing |
| `arb_primitive_type()` | FFI primitive TypeDesc variants (I8..Ptr) | FFI type testing |
| `arb_type_desc(depth)` | Primitive + compound types (Struct, Array) | FFI compound type testing |
| `arb_flat_struct()` | StructDesc with 1-6 primitive fields | FFI struct testing |
| `arb_value_for_type(desc)` | Value matching a given TypeDesc | FFI roundtrip testing |
| `arb_typed_value()` | (TypeDesc, Value) pair where value matches type | FFI type/value pair testing |
| `arb_struct_and_values()` | (StructDesc, Value::array) pair | FFI struct marshalling testing |

Some property test files define local strategies for their domain (e.g.,
`reader.rs` defines `arb_source()` for generating valid Elle source code,
`strings.rs` defines `arb_unicode_string()`).

## How to add a new test

### Adding an integration test

1. Create `tests/integration/myfeature.rs`
2. Add to `tests/integration/mod.rs`:
   ```rust
   mod myfeature {
       include!("myfeature.rs");
   }
   ```
3. In the test file, import the shared helper:
   ```rust
   use crate::common::eval_source;
   use elle::Value;

   #[test]
   fn test_my_feature() {
       assert_eq!(eval_source("(my-feature 42)").unwrap(), Value::int(42));
   }
   ```

### Adding a property test

1. Create `tests/property/myfeature.rs`
2. Add to `tests/property/mod.rs`:
   ```rust
   mod myfeature {
       include!("myfeature.rs");
   }
   ```
3. In the test file:
   ```rust
   use crate::common::eval_source;
   use elle::Value;
   use proptest::prelude::*;

   proptest! {
       #![proptest_config(ProptestConfig::with_cases(200))]

       #[test]
       fn my_invariant(n in -1000i64..1000) {
           let result = eval_source(&format!("(my-fn {})", n)).unwrap();
           prop_assert_eq!(result, Value::int(n));
       }
   }
   ```

### Adding a unit test

1. Create `tests/unittests/mymodule.rs`
2. Add to `tests/unittests/mod.rs`:
   ```rust
   mod mymodule {
       include!("mymodule.rs");
   }
   ```
3. Import Rust APIs directly — no need for `eval_source` unless testing
   through the pipeline.

### Adding an inline test

Add a `#[cfg(test)]` module at the bottom of the `src/` file you're testing.
This gives access to private items. No registration needed.

### Naming conventions

- Test files: lowercase, hyphenated concepts joined with underscores
  (e.g., `closures_and_lambdas.rs`, `effect_enforcement.rs`)
- Test functions: `test_` prefix for example-based, descriptive name for
  property tests (e.g., `fn int_roundtrip(...)`, `fn add_commutative(...)`)
- Property test names describe the invariant, not the implementation

### Registration

All test files in `tests/` subdirectories must be registered in their
`mod.rs` using the `include!()` pattern:

```rust
mod myfile {
    include!("myfile.rs");
}
```

This is required because `tests/lib.rs` uses `include!()` to pull in the
subdirectory `mod.rs` files. Without registration, the test file will be
ignored.

Inline `#[cfg(test)]` modules in `src/` need no registration.

## Property test conventions

### Case counts

Choose case counts based on the cost of each test case:

| Cost per case | Cases | Example |
|---------------|-------|---------|
| Cheap (no eval, pure Rust) | 1000 | NaN-boxing roundtrips, effect combine laws |
| Medium (single eval) | 200 | Arithmetic properties, reader roundtrips |
| Expensive (multiple evals or recursion) | 50-100 | Bug regression, determinism, complex programs |

Set via `#![proptest_config(ProptestConfig::with_cases(N))]` inside the
`proptest!` block.

### Writing new generators

- For Elle source code generation, build format strings with generated
  parameters: `format!("(+ {} {})", a, b)`. This is simpler and more
  maintainable than generating ASTs.
- For Value generation, use the strategies in `strategies.rs` or compose
  new ones from proptest primitives.
- Bound recursive generators with a depth parameter to prevent explosion.
- Weight leaf values higher than compound values in `prop_oneof!` to keep
  test cases manageable.
- Use `prop_filter` or `prop_assume!` to exclude invalid inputs rather than
  generating only valid ones (when the invalid space is small).

### Structure

Property test files follow a consistent structure:

1. Module-level comment explaining what invariants are tested
2. Any local helper functions (e.g., `infer_effect()` in `effects.rs`,
   `syntax_eq()` in `reader.rs`)
3. `proptest!` blocks grouped by invariant category, separated by section
   headers (`// =========================================================================`)
4. Non-property `#[test]` functions at the bottom for constant/edge cases
   that don't need generation

## Running tests

```bash
# Full test suite (includes inline tests, integration, unit, property, examples)
cargo test --workspace

# Just the main crate (no workspace members)
cargo test

# Specific test by name (substring match)
cargo test test_name

# All tests in a category
cargo test property::          # All property tests
cargo test integration::       # All integration tests
cargo test unittests::         # All unit tests
cargo test vm::                # All VM tests

# All tests in a specific file
cargo test property::arithmetic::    # All arithmetic property tests
cargo test integration::fibers::     # All fiber integration tests

# Run all examples as tests
cargo test --test '*'

# Run with output (for debug tests that use println!)
cargo test test_name -- --nocapture

# Run a single example file
cargo run -- examples/closures.lisp
```

## Fixtures

`tests/fixtures/` contains static data files used by tests:

- `naming-good.lisp` — Elle source with correct kebab-case naming (lint passes)
- `naming-bad.lisp` — Elle source with camelCase/PascalCase/snake_case naming
  (lint flags warnings)

Used by `integration/lint.rs` to test the linter against real files.
