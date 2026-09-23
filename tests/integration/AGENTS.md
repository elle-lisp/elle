# tests/integration

<!-- audited: 2026-09-22 -->

Full-pipeline integration tests: end-to-end behavior verification.

## Responsibility

Test end-to-end pipeline behavior by evaluating Elle source code through the
full pipeline (Reader → Expander → Analyzer → Lowerer → Emitter → VM) and
checking the result.

Does NOT:
- Test individual modules in isolation (that's unit tests)
- Test invariants across random inputs (that's property tests)
- Test Elle scripts (that's `tests/elle/`)

## Finding a test

`mod.rs` lists every file that runs, and each file opens with a call-out saying
what it covers. Read the two together; there is no third list here to consult,
because a hand-kept copy of the directory goes stale the week somebody adds a
file and it then sends readers to tests that no longer exist.

Three groups are worth knowing about, because their names do not say that they
check the repository rather than the language:

| Group | Files |
|-------|-------|
| The documents and their policy | `agents.rs`, `audit.rs`, `prose.rs`, `paths.rs`, `bytecode_doc.rs`, `doctest.rs`, `rustsource.rs` |
| CI and the corpus runner | `workflows.rs`, `run_artifacts.rs`, `plugins.rs`, `budget.rs`, `runner_budget.rs`, `capacity.rs`, `profiles.rs`, `truncation.rs`, `runner_exit_trap.rs`, `timeout_capture.rs`, `runner_gauges.rs`, `measurements.rs`, `isolation.rs`, `state_dir.rs`, `run_identity.rs`, `import.rs`, `form_profile.rs`, `boot_fingerprint.rs` |
| CLI surfaces | `argv_cli.rs`, `dump_cli.rs`, `flip_cli.rs`, `tier_cli.rs`, `version.rs`, `dispatch.rs`, `repl_exit_codes.rs` |

`allocator.rs` sits in the directory unregistered and does not compile; the
comment at the foot of `mod.rs` says why.

## Key patterns

### Basic test structure

```rust
use crate::common::eval_source;
use elle::Value;

#[test]
fn a_call_reaches_the_primitive() {
    eval_source("(my-feature 42)", |r| assert_eq!(r.unwrap(), Value::int(42)));
}
```

`eval_source` hands the result to a closure and runs it while the `Runtime`'s
heap is still alive. A heap-valued result dangles past teardown otherwise, so
the closure is not a style choice.

### Testing errors

```rust
#[test]
fn an_unbound_name_says_so() {
    eval_source("(undefined-function)", |result| {
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("undefined"));
    });
}
```

### Direct VM access

`setup()` answers a `Runtime`. Take the three disjoint borrows from it with
`parts()`:

```rust
use crate::common::setup;

#[test]
fn the_vm_answers_directly() {
    let mut rt = setup();
    let (vm, symbols, cctx) = rt.parts();
    let result = elle::pipeline::eval("(+ 1 2)", symbols, vm, cctx, "<test>");
    assert_eq!(result.unwrap(), elle::Value::int(3));
}
```

### Driving the binary

A flag, an exit code or anything `main` decides needs the binary, not the
library. Spawn it by its Cargo-supplied path:

```rust
let out = std::process::Command::new(env!("CARGO_BIN_EXE_elle"))
    .args(["--version"])
    .output()
    .expect("spawn elle");
```

## Naming conventions

- Test files: lowercase words joined with underscores (`signal_enforcement.rs`,
  `trace_isolation.rs`). A file driving one CLI flag is named for it and ends
  `_cli.rs`.
- Test functions: a sentence naming the claim the body proves
  (`a_hot_function_compiles_with_no_flag_at_all`). The older `test_` prefix
  survives in about a third of the suite and says nothing a reader needs.
- Property test names describe the invariant, not the implementation.

## Registration

A file here is not compiled until `mod.rs` names it, so an unregistered file is
a test suite that reports success having run nothing. Add it:

```rust
mod myfile {
    include!("myfile.rs");
}
```

The `include!()` shape is what `tests/lib.rs` needs to pull the directory in as
one crate.

## Invariants

1. **Tests are independent.** Each test creates a fresh VM (`eval_source`) or
   uses a cached VM with restored globals (`eval_reuse`). No cross-test
   contamination.

2. **Tests use the full pipeline.** `eval_source` runs Reader → Expander →
   Analyzer → Lowerer → Emitter → VM, so a test here verifies end-to-end
   behavior rather than one component.

3. **Error tests check the message.** `result.unwrap_err().contains(...)` —
   `is_err()` alone passes when the run fails for a reason the test never
   meant to cover.

4. **Tests are deterministic.** The same source gives the same result. No
   randomness, and no timing dependency outside `time_property.rs` and
   `time_elapsed.rs`.

## Common pitfalls

- **`eval_source` in a property test.** It builds a fresh VM per case, which is
  slow. Use `eval_reuse` or `eval_reuse_bare`.
- **A scratch path under `/tmp`.** Derive it from `std::env::temp_dir()` and
  give it a unique name; `scratch.rs` fails the build over this.
- **Racing the JIT worker.** Compilation runs on the `elle-jit` thread, so
  `(jit? f)` after a hot loop is a race. `--trace=syncjit` compiles on the VM
  thread and makes the answer deterministic.
- **Forgetting to register a new file.** It is silent, and it looks like a pass.
