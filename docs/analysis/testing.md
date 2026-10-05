# Testing Strategy

<!-- audited: 2026-10-04 -->

Which *kind* of test to write, and where it belongs.

> For how the test systems work and how to run them — the agent-first runner
> (`elle test`), the session DB, `make smoke`/`make test`, reading results — see
> [docs/testing.md](../testing.md). For the Rust suite's helpers and every
> `cargo test` recipe, see [tests/AGENTS.md](../../tests/AGENTS.md). This
> document is the **decision tree**: given a thing to test, which suite, and
> which kind of test in it?

## Test execution order

`make test` gates in this order; fail fast, cheapest first:

| Step | What | Purpose |
|------|------|---------|
| 1 | `make qa` | rustfmt, clippy, the macOS and Android cross-checks, rustdoc, and the Rust doctests |
| 2 | `make smoke` | The language suite, the implementation suite on the rig, the doctests, the embedding demo and the surface gate |
| 3 | `cargo test --workspace --lib --all-features` | The unit tests, inline beside the code they test |
| 4 | `cargo test --test '*' -- --skip property` | The integration tests and the standalone test binaries |
| 5 | `cargo test -p elle-rig` | The rig's own tests |

`qa` takes about two minutes and `smoke` about thirty, so a formatting or
clippy failure stops the gate before the suites start.

`make test` runs one build. The other builds that must pass the language suite
are CI jobs ([ci](ci.md)); run one locally by building it and running `make
smoke-lang` against it.

The Elle suites are the cheapest full-pipeline check: reader, expander,
analyzer, lowerer, emitter, VM, JIT, and a broad swath of primitives. If one
fails, the session DB names every failing file (`elle test --summary`).

Integration tests are slower because they require Rust-level setup (VM
construction, symbol table initialization, error message inspection).

Property tests are the slowest, because each one runs many generated cases.
`make test` passes `--skip property` at step 4 and leaves them to CI, which
sets its own case count per job. Run them by hand with `cargo test property::`.


## Decision tree


For any test you need to write, answer these questions in order:

**0. Would every correct implementation of Elle pass this test?**

A correct implementation may run on other tiers, allocate differently, inline
differently, and name its internals differently. So a test passes question 0
only when it asserts what the language promises — values, errors, signals,
fiber states, and whether a program compiles — and reads nothing else
([spec](../spec.md) § Two suites).

→ **Yes: a language test.** It goes in
[tests/lang](../../tests/lang/overview.md), runs on every build with no flag,
and calls no implementation extension. Continue at question 2 to decide its
shape.

→ **No: an implementation test.** It reads a gauge, observes or chooses a
tier, reads a compiler artifact, needs a mode such as the JIT off or
`--trace=guardfree`, or its subject is a mechanism — a calling convention, a
tail move, an owned parameter, a region release. Write it as an Elle file in
[tests/impl](../../tests/impl/overview.md), with a sidecar for the mode it
needs ([rig](../../rig/overview.md)), or as a Rust test when it needs a Rust
type or an input built beneath the compiler. Continue at question 1.

A file that holds both kinds of assertion is two files. Split it: the language
claims go to `tests/lang/`, the rest to `tests/impl/`.

**1. Does the test need access to Rust types, APIs, or compiler internals?**

Examples: inspecting `HirKind` variants, checking `Signal` values, calling
`analyze()` or `compile()` directly, testing `Value` constructors, examining
`Lexer`/`Reader` output, verifying bytecode disassembly, testing JIT internals.

→ **Rust test.** Go to "Which Rust test category?" below.

**2. Does the test assert that something fails at compile time?**

Code that should be rejected by the analyzer or lowerer before the VM ever
runs — undefined variables, break across function boundaries, invalid
destructuring syntax, arity mismatches at known call sites.

→ **Elle file**, when the rejection and its message are all the test needs.
Whether a program compiles is a language claim, so the file goes in
`tests/lang/`. Compile the source text inside `protect` and assert that it
fails:

```lisp
(def [ok? err] (protect (compile/whole-module "(def x 1) (assign x 2)" "<doc>")))
(assert (not ok?) "assigning an immutable binding does not compile")
(assert (= (get err :error) :compile-error))
(assert (string/contains? (get err :message) "cannot assign immutable binding"))
```

→ **Rust integration test**, when the assertion needs a Rust type the error
carries, or a runtime Elle cannot build. Call `eval_source(input, |r| ...)`
and inspect the error inside the closure, while the runtime that produced it
is still alive.

A Unicode generation test is the second case: the runner's in-process mode
compiles every file on its own shared VM, which uses the default generation,
so a file that selects another generation with `(unicode! N)` answers
differently there than under `elle FILE`. Build a `Runtime::with_unicode(...)`
in a Rust integration test instead.

**3. Does the test evaluate Elle source and check the resulting value?**

This is the vast majority of tests. In Rust the pattern is
`eval_source("(some-expr)", |r| assert_eq!(r.unwrap(), Value::int(42)))`.

→ **Elle file** in [tests/lang](../../tests/lang/overview.md) for a language
test, or [tests/impl](../../tests/impl/overview.md) for an implementation
test. Translate to `(assert (= (some-expr) 42) "description")`.

**4. Does the test verify a runtime error?**

→ **Elle file.** `protect` answers the error as a value, so the file checks its
kind, and its message with `string/contains?`:

```lisp
(def [ok? err] (protect (/ 1 0)))
(assert (not ok?) "division by zero should error")
(assert (= (get err :error) :division-by-zero) "error kind")
(assert (string/contains? (get err :message) "division by zero"))
```

**5. Does the test use random input generation to find bugs?**

Property tests use proptest to generate random inputs and verify that an
invariant holds across all of them. This is valuable when randomness genuinely
finds bugs that concrete cases would miss — for example, testing that a
roundtrip property holds for all possible values, or that a mathematical law
(like commutativity) holds across all inputs.

However, if you're really just testing a fixed set of known-good examples
("yield 3 values, resume 3 times, get them back in order"), property
testing is the wrong tool. Write Elle files instead — they're faster and
clearer.

→ **Property test** in [tests/property/](../../tests/property/) IF random
generation genuinely adds value. Otherwise, write Elle files.


## Which Rust test category?


| Need | Location | When |
|------|----------|------|
| Access to private items (`pub(crate)` or less) | Inline `#[cfg(test)]` in the source file | Testing implementation details of a single module |
| Access to public Rust APIs, no pipeline | [tests/unittests/](../../tests/unittests/) | Testing `Value`, `SymbolTable`, primitives via Rust calls |
| Access to intermediate pipeline stages | [tests/integration/](../../tests/integration/) | Testing `analyze()`, `compile()`, HIR/LIR structure, signals |
| Compile-time rejection that needs a Rust type | [tests/integration/](../../tests/integration/) | An error field, a span, or a runtime Elle cannot build |
| Invariants across generated inputs | [tests/property/](../../tests/property/) | Property-based tests with proptest |
| Reads or perturbs process-global state | A file of its own directly under [tests/](../../tests/) | A process-wide counter, an rlimit, a signal disposition, a re-exec, or a fault the harness must survive |
| Private items, and a process of its own | Inline `#[cfg(test)]`, with the body run by `crate::io::isolate::run` | A signal sent to the whole process, a fault, a change to descriptor 0 |

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

### An inline test that needs a process runs its body in a fresh one

An inline test cannot move to a binary of its own, because it reaches private
items. Some still need a process to themselves. A signal sent with
`kill(getpid(), …)` reaches any thread that leaves it unblocked, and libtest's
threads leave it unblocked.

Such a test passes its body to `crate::io::isolate::run`. Do not call
`libc::fork` from a test. A fork copies every lock in the process as it stood at
that instant, and a lock that another thread held stays held in the child. That
thread does not exist in the child, so a child that takes the lock waits
forever.

The test cannot know which locks those are. The standard library's SIGSEGV
handler takes a lock that every thread takes when it starts and when it ends,
and libtest starts a thread for every test. The crate's own statics hold more,
and a sibling test may hold any of them at the instant of the fork.

`run` starts the test binary again, on the calling test alone. In that copy,
libtest's main thread waits and no other test runs, so no thread holds a lock.
The test's thread forks there, and the body runs as the only thread of its
process. The copy reports what the body did on its stdout. A copy that reports
nothing ran no test of that name, and `run` fails the test.

The test's own code before the call runs twice, once in each process. So call
`run` first. [forks.rs](../../tests/integration/forks.rs) is the standing check
that no other file calls `libc::fork`.


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
in [tests/AGENTS.md](../../tests/AGENTS.md). An Elle file gates itself in-file
and reports through the runner, and an implementation test names its mode in a
sidecar; [docs/testing.md](../testing.md) covers both.

An inline `#[cfg(test)]` module needs no registration. Add it at the bottom of
the `src/` file it tests, where it reaches that file's private items.


---

## See also

- [Analysis index](index.md)
