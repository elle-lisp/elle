# Testing Strategy

<!-- audited: 2026-09-09 -->

Which *kind* of test to write, and where it belongs.

> For how the test systems work and how to run them — the agent-first runner
> (`elle test`), the session DB, `make smoke`/`make test`, reading results — see
> [docs/testing.md](../testing.md). For the Rust suite's helpers and every
> `cargo test` recipe, see [tests/AGENTS.md](../../tests/AGENTS.md). This
> document is the **decision tree**: given a thing to test, is it an Elle
> script or a Rust test, and which kind?

## Test execution order

`make test` gates in this order; fail fast, cheapest first:

| Tier | What | Purpose |
|------|------|---------|
| 1 | `make smoke` | The Elle corpus across the vm and jit policies, then the doctests and the embedding demo |
| 2 | `make smoke-nouring` | The corpus again on the thread-pool I/O backend |
| 3 | `make qa` | rustfmt, clippy, the macOS cross-check, rustdoc, and the workspace doctests |
| 4 | `cargo test --workspace --lib` | The unit tests, inline beside the code they test |
| 5 | `cargo test --test '*'` | The integration tests and the standalone test binaries |

The Elle corpus is the cheapest full-pipeline check: reader, expander, analyzer,
lowerer, emitter, VM, JIT, and a broad swath of primitives. If it fails, the
session DB names every failing form (`elle test --summary`).

Integration tests are slower because they require Rust-level setup (VM
construction, symbol table initialization, error message inspection).

Property tests are the slowest, because each one runs many generated cases.
`make test` passes `--skip property` at tier 5 and leaves them to CI, which
sets its own case count per job. Run them by hand with `cargo test property::`.


## Decision tree


For any test you need to write, answer these questions in order:

**1. Does the test need access to Rust types, APIs, or compiler internals?**

Examples: inspecting `HirKind` variants, checking `Signal` values, calling
`analyze()` or `compile()` directly, testing `Value` constructors, examining
`Lexer`/`Reader` output, verifying bytecode disassembly, testing JIT internals.

→ **Rust test.** Go to "Which Rust test category?" below.

**2. Does the test assert that something fails at compile time?**

Code that should be rejected by the analyzer or lowerer before the VM ever
runs — undefined variables, break across function boundaries, invalid
destructuring syntax, arity mismatches at known call sites.

→ **Rust integration test.** The code cannot be run as an Elle script because
it does not compile. Call `eval_source(input, |r| ...)` and inspect the error
message inside the closure, while the runtime that produced it is still alive.

The same rule covers Unicode generation tests: corpus files compile on the
shared runner VM, which uses the default generation, so a file that selects
another generation with `(unicode! N)` cannot join the corpus. Build a
`Runtime::with_unicode(...)` in a Rust integration test instead.

**3. Does the test assert that something fails at runtime and need to inspect
the error message for specific content?**

Example: checking that a division-by-zero error message contains
"division by zero", or that an undefined variable error includes the
variable name.

→ **Rust integration test IF** the assertion requires substring matching on
the error message that `try`/`catch` in Elle cannot express. If the test only
needs to confirm that an error occurs (not inspect its message), it can be
Elle — use `protect` and check the error kind keyword.

**4. Does the test evaluate Elle source and check the resulting value?**

This is the vast majority of tests. The pattern is:
`eval_source("(some-expr)", |r| assert_eq!(r.unwrap(), Value::int(42)))`.

→ **Elle test script** in [tests/elle/](../../tests/elle/). Translate to:
`(assert-eq (some-expr) 42 "description")`.

**5. Does the test verify a runtime error occurs (not a compile error)
and only needs to check the error kind, not the full message?**

Example: confirming division by zero signals an error with kind
`:division-by-zero`.

→ **Elle test script.** Use `protect`:
```
(def [ok? err] (protect (/ 1 0)))
(assert-false ok? "division by zero should error")
(assert-eq (get err :error) :division-by-zero "error kind")
```

**6. Does the test use random input generation to find bugs?**

Property tests use proptest to generate random inputs and verify that an
invariant holds across all of them. This is valuable when randomness genuinely
finds bugs that concrete cases would miss — for example, testing that a
roundtrip property holds for all possible values, or that a mathematical law
(like commutativity) holds across all inputs.

However, if you're really just testing a fixed set of known-good examples
("yield 3 values, resume 3 times, get them back in order"), property
testing is the wrong tool. Write Elle test scripts instead — they're faster
and clearer.

→ **Property test** in [tests/property/](../../tests/property/) IF random
generation genuinely adds value. Otherwise, write Elle test scripts.


## Which Rust test category?


| Need | Location | When |
|------|----------|------|
| Access to private items (`pub(crate)` or less) | Inline `#[cfg(test)]` in the source file | Testing implementation details of a single module |
| Access to public Rust APIs, no pipeline | [tests/unittests/](../../tests/unittests/) | Testing `Value`, `SymbolTable`, primitives via Rust calls |
| Access to intermediate pipeline stages | [tests/integration/](../../tests/integration/) | Testing `analyze()`, `compile()`, HIR/LIR structure, signals |
| Compile-time rejection | [tests/integration/](../../tests/integration/) | Code that must not compile |
| Runtime error message inspection | [tests/integration/](../../tests/integration/) | Substring matching on error strings |
| Invariants across generated inputs | [tests/property/](../../tests/property/) | Property-based tests with proptest |
| Reads or perturbs process-global state | A file of its own directly under [tests/](../../tests/) | A process-wide counter, an rlimit, a signal disposition, a re-exec, or a fault the harness must survive |

For Rust integration tests that don't call stdlib functions (map, filter,
fold, etc.), prefer `eval_source_bare` over `eval_source` — it skips stdlib
initialization and is faster. Prelude macros (defn, let*, ->, etc.) are
still available with `eval_source_bare`.

### Process-global state needs its own binary

Everything under [tests/lib.rs](../../tests/lib.rs) — `unittests/`,
`integration/`, `property/` — compiles into ONE binary, and libtest runs its
tests concurrently on many threads. A test there shares the process with a
thousand others.

That is fine for a test whose subject is a value it owns. It is wrong for a
test whose subject is a process-wide fact, because a concurrent sibling
changes that fact underneath the measurement and the test cannot tell the
two apart. A page counter reads a sibling's allocation as its own leak. An
rlimit or a signal disposition one test installs stays installed for every
test that follows it. A deliberate fault takes the whole binary down, and
with it every unrelated test.

Give such a test its own file directly under [tests/](../../tests/). Cargo
builds one binary per file there, so the process holds only that test and
whatever it starts. Name the file for the subject
([worker_heap.rs](../../tests/worker_heap.rs),
[region_process_teardown.rs](../../tests/region_process_teardown.rs)), and say
in its header which global it reads — that is the fact the next reader needs
and cannot see from the assertion.

A test that merely runs slowly does not qualify. The cost is a whole extra
link of the crate per file, so the reason must be the shared process itself.


## A `#[should_panic]` test names the profile it runs in

`debug_assert!` and `#[cfg(debug_assertions)]` compile to nothing in a release
build. A test that expects a panic from one of them passes under `cargo test`
and fails under `cargo test --release -p elle --lib`, where libtest reports
"test did not panic as expected" and names the test's own line.

Gate such a test on the condition that compiles the check it drives:

```rust
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "stale region")]
fn stale_value_deref_panics_after_region_free() {
```

A test whose panic comes from `assert!`, `panic!` or `.expect()` takes no gate.
Those compile in both profiles, and the test is worth more running in both.

[tests/integration/profiles.rs](../../tests/integration/profiles.rs) is the
standing check. It reads the panic sites out of `src/`, and it fails when an
ungated `#[should_panic]` names one that only a debug build compiles. Give
every such test an `expected` message: a bare `#[should_panic]` names no panic,
so nothing can say which profile raises the one it waits for.

No CI job builds a release test harness, so this profile runs only when
somebody runs it. It is worth keeping green because some state exists only
there: a counter kept beside an assert the release profile removes can be read
no other way, and reading it against a suite that is already red hides the
answer.


## Adding a new test

Once the tree above has named a category, the steps for it — the directory, the
`include!` registration, the helper to import, the proptest configuration — are
in [tests/AGENTS.md](../../tests/AGENTS.md). An Elle script gates itself in-file
and reports through the runner; [docs/testing.md](../testing.md) covers that.

An inline `#[cfg(test)]` module needs no registration. Add it at the bottom of
the `src/` file it tests, where it reaches that file's private items.


---

## See also

- [Analysis index](index.md)
