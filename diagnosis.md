# Diagnosis: JIT region use-after-free class (fiber/IO escape retains)

Working handoff for a clean session. Delete when the fix lands.

## TL;DR

Raising the dependency opt-level (`[profile.dev.package."*"] opt-level = 3`)
makes Cranelift compile ~600× faster, so under eager JIT a hot function wins the
background-compile-vs-execute race and runs as native code across a wider slice
of execution. That timing shift exposes **two** independent JIT bugs:

1. **Fuel-preemption dropped frame — FIXED** (commit `2f512185`). A fuel-limited
   fiber whose body ran as JIT code lost its callee's suspended frame. Pinned by
   `tests/elle/fuel-jit-preempt.lisp`. This was the original `fuel-apply-fold`
   failure. Done, green on `make smoke`.

2. **Region UAF class — UNSOLVED, this document.** A value that escapes a
   fiber/IO subtree via the JIT tail-call / return path is missing the
   cross-region incref the interpreter applies. When the subtree's owning region
   is torn down (a fiber/io-request decref during a resume), the teardown
   **cascade-frees** the escaped value's region while a JIT continuation still
   holds it → deref → panic.

The override **cannot land** until #2 is fixed (`origin/main` stays green — the
"not rocket science rule", see CONTRIBUTING.md). #1 is landed regardless.

## Committed so far (branch `s14`)

- `2f512185` — jit: preserve a fuel-suspended callee's frame across the JIT tier.
  Adds `VM::park_suspended_callee_frame` (`src/vm/call/inner.rs`), routes the four
  JIT sites that run an interpreter callee via `execute_bytecode_saving_stack`
  through the shared `interp_exec_result_to_jit_value`
  (`src/jit/calls/callops/arraycall.rs`), plus `tests/elle/fuel-jit-preempt.lisp`.
- `1b334bbc` — region: attribute a stale-region-deref UAF to its premature free.
  Wires `freelog::describe` into the region-generation guard
  (`src/value/fiberheap/regionstore/pointer.rs`) and extends `describe` with a
  **cascade-root walk** (`src/value/fiberheap/freelog.rs`) — this is the tooling
  that localized bug #2. Debug-only; only enriches a panic message.

The `opt-level = 3` override is NOT applied (reverted). Its opt-3 dep artifacts
are cached, so re-applying + `cargo build -p elle` is ~4s (NOT a ~10 min opt-0
rebuild — keep the override applied for the whole debug session).

## Reproduce (deterministic enough: aborts within a few trials)

```sh
# 1. Apply the override (before [profile.release] in Cargo.toml):
#      [profile.dev.package."*"]
#      opt-level = 3
# 2. Fast rebuild (deps cached at opt-3):
cargo build -p elle
# 3. Loop the runner on the culprit file with free-logging:
for i in $(seq 1 8); do
  ./target/debug/elle test tests/elle/process-io.lisp --trace=free,freebt 2>&1 \
    | grep -A30 "stale region deref" && break
done
```

Critical facts about reproduction:

- **Must go through the runner** (`elle test`), NOT a direct
  `elle --jit=eager process-io.lisp`. The runner runs the multi-form file under
  the `[jit]` (`:eager`) whole-file policy in an `os/spawn` worker with a pumped
  scheduler (`src/test.lisp`, `exec-source-capture` / `whole-file-policies`), so
  `print`/socket I/O actually yields. Direct runs use a different execution model
  and do NOT reproduce.
- **`--trace=guardfree` masks it** (mprotect-per-free perturbs the race enough to
  hide it). `--trace=free` and `--trace=free,freebt` are cheap enough to still
  reproduce AND populate the free-log. Use those.
- **Warm + `(jit/rejections)` drain does NOT reproduce it.** Pre-draining forces
  the *fully-JIT* path; the bug needs the async mid-execution interpreter→JIT
  switchover, i.e. a partial-JIT window. (This is the OPPOSITE of the fuel bug,
  where drain-forcing DID reproduce — do not reuse that trick here.)
- The culprit file is `tests/elle/process-io.lisp` (tests 10–12 use
  `http2:serve`/`http2:connect`/`http2:close` inside `process:start`). It also
  reproduces in the 10-file process-family batch and the full corpus, but
  process-io alone is the tightest oracle.

## The deref (where it panics)

Region-generation guard fires: `src/value/fiberheap/regionstore/pointer.rs`
(`stale region deref: … region R generation 0, but generation 1 is current`).
The panic aborts (`fatal runtime error: failed to initiate panic, error 5`)
because it unwinds into `extern "C" elle_jit_call` — it IS a real panic, it just
can't unwind across the C ABI frame.

Deref backtrace (stable across runs):

```
region_of_ptr (pointer.rs)                     ← generation guard
region::region_of_ptr → arena::region_of
vm::core::region::dispatch_native_call  region.rs:206   ← the #[cfg(debug)] declaration
                                                          oracle calls region_of on a
                                                          native call's RESULT value
vm::call::inner::call_inner            inner.rs:100
vm::call::handle_call                  call.rs:66
dispatch_instruction (Call arm)        opcodes.rs:130
execute_bytecode_inner_impl → trampoline_loop
execute_bytecode_saving_stack          execute.rs:337
elle_jit_call                          callops.rs:178   ← JIT tail-call sentinel resolution
```

So: a JIT function's TailCall resolves to an interpreter callee C (via the
sentinel path at `callops.rs:178`); C makes a native call; the declaration
oracle's `region_of(result)` derefs a region that has already been freed.

The oracle (`region.rs:201-234`) is debug-only; it is not the bug, just the
detector. In release the deref would silently read freed memory.

## The free (root cause, from the cascade-root walk)

The freed value's page was reclaimed via a **cascade**. With the new tooling the
attribution reads (example run):

```
free #… region X via cascade(Y) — unknown
  ↳ cascade root: free #… region Y (direct) — DecrefValueRegion of fiber
      (runtime region Y) @ lib/http2.lisp:312:7
        at src/vm/dispatch/region.rs:35 (set_reason for the decref)
        at opcodes.rs:424 (DecrefValueRegion dispatch)
        at execute.rs:298 execute_bytecode_from_ip
        at src/vm/core/resume.rs:173
        at src/vm/fiber/resume.rs:248  do_fiber_subsequent_resume
        at src/vm/fiber/resume.rs:153
        at src/vm/fiber/child.rs:53
        at src/vm/fiber/resume.rs:147
        at src/vm/fiber/trampoline.rs:135
```

Two DIFFERENT cascade roots have been observed across runs (so it is a class,
not one line):

- `DecrefValueRegion of fiber @ lib/http2.lisp:312:7` — `http2:close` releasing a
  reader/writer fiber value at its last use, during a fiber resume.
- `DecrefValueRegion of io-request @ lib/process.lisp:872:25` — the process
  scheduler's `@complete-io` doing `(fiber/resume f (get completion :value))`.

Both roots fire **during a fiber resume** (`do_fiber_subsequent_resume` →
`execute_bytecode_from_ip`). Freeing the fiber/io-request region **cascades**
(subtree teardown) to free a child region whose value has escaped into a live
JIT continuation.

`cascade(Y)` = region Y's own free reached this region as part of Y's subtree
teardown (see `src/value/fiberheap/regionstore/free.rs`, `free_region_set`).
`freelog.rs` header: a **cascade** free means a *missing cross-region incref on
the referrer* (vs a *direct* free = a liveness bug). This one is a cascade.

## Mechanism / hypothesis

The interpreter keeps an escaped value alive across fiber operations through
several retains that the JIT path is (partially) missing:

- `handle_primitive_signal` escape retains (`src/vm/signal.rs`) vs the JIT's
  `jit_handle_primitive_signal` (`src/jit/calls.rs:68-114`). The Suspend arm
  already has a `SuspendEscape` retain (`region_of`, not `result_region_of`, at
  `calls.rs:91-112`) — added for `tests/elle/region-jit-io-suspend-uaf.lisp`, the
  sibling bug. The Resume arm routes to `handle_fiber_resume_signal_jit`.
- `prim_fiber_resume` (`src/primitives/fibers.rs:127`) stores the resume value
  into `f.signal` **without incref** (line 175). Before it, `release_parked_signal`
  (`src/vm/fiber/refcount.rs:159`) has the KEY subtlety: when the resume value
  **shares the io-request's region** (the "Fresh io-op" case — the completion
  buffer is built in the request's region), it deliberately does NOT decref that
  region and leaves it to the **caller's** `DecrefValueRegion` (the one at
  `process.lisp:872`). So the resume value handed to `f` is a *borrow* of a region
  the caller still owns and will free.
- `handle_fiber_resume_signal_jit` (`src/vm/fiber/jit.rs`) runs the child
  synchronously; on a child re-yield it builds a `FiberResume` frame and returns
  `YIELD_SENTINEL`.
- The JIT tail-call MOVE: `elle_jit_tail_call` → `build_tail_call_env`
  (`own_params=false`, pure MOVE) in `src/jit/calls/callops/arraycall.rs` and
  `src/vm/env.rs`. Note the `deferred_release_region: None` "no-regression
  placeholder" comment there — closure-callee adoption is NOT wired on the JIT
  tail path (the interpreter does it in `tail_call_inner`,
  `src/vm/call/inner/tail.rs`).

**Working hypothesis:** a value V escaping a fiber/IO subtree into a JIT
continuation (via the JIT tail-call MOVE, a JIT return, or the resume-value
borrow that shares a region) is not incref'd for that cross-region edge on the
JIT path. When the subtree's owner region is torn down during a resume, V's
region cascade-frees while the JIT continuation still holds V. The fix is a
cross-region incref at the escape site — but it must be the RIGHT site (balanced,
no leak) and must cover BOTH observed roots (io-request-shared-region and
fiber-subtree-child). Do not guess: identify exactly where V escapes.

## Ruled out

- **Not the fuel fix (`2f512185`).** On the non-suspend tail-call path,
  `interp_exec_result_to_jit_value` is byte-identical to the old
  `exec_result_to_jit_value`; the deref is during a callee's *fresh* execution,
  not a resume of a parked frame. And `no-override + full fix` passed `make smoke`
  including the entire `region-*` UAF suite.
- **Not a codegen difference from optimizing cranelift.** The JIT pins its own
  output at `opt_level="speed"` (`src/jit/compiler.rs`); the override only changes
  compile *speed*. Same override binary passes `elle --jit=eager process-io` and
  fails `elle test [jit]` — identical code, different harness timing.

## Validation plan for a candidate fix

1. `process-io.lisp` oracle (override applied): run ~30× under `--trace=free`,
   expect 0 aborts.
2. `make smoke` WITHOUT the override → green.
3. `make smoke` WITH the override → green (this is the real gate; the whole point
   is that the faster-JIT timing must be clean corpus-wide).
4. Leak oracle: `tests/elle/oracle.lisp` shows no new leaks (an over-incref fix
   leaks; the oracle + `region-*-leak.lisp` tests catch it).
5. `cargo test` (region ownership tests under `src/runtime/tests/ownership/`).

If the fix lands: apply the override in `Cargo.toml`, update
`docs/impl/wasm.md` "Debug builds …" section (reframe as "override applied; JIT
tier behavior-identical because output is pinned to `opt_level=speed`; invariant
pinned by `fuel-jit-preempt.lisp`"), and delete this file.

## Key files

| Area | Path |
|------|------|
| Deref/detector | `src/value/fiberheap/regionstore/pointer.rs` (guard), `src/vm/core/region.rs:206` (oracle) |
| Cascade teardown | `src/value/fiberheap/regionstore/free.rs` (`free_region_set`) |
| Free-log tooling | `src/value/fiberheap/freelog.rs` (`describe` + `cascade_root`) |
| JIT tail-call sentinel | `src/jit/calls/callops.rs:175` and `elle_jit_resolve_tail_call` |
| JIT tail-call MOVE | `src/jit/calls/callops/arraycall.rs` (`elle_jit_tail_call`), `src/vm/env.rs` (`build_tail_call_env`) |
| Interpreter tail reference | `src/vm/call/inner/tail.rs` (`tail_call_inner`) |
| JIT signal escape | `src/jit/calls.rs:68` (`jit_handle_primitive_signal`) vs `src/vm/signal.rs` (`handle_primitive_signal`) |
| JIT fiber resume | `src/vm/fiber/jit.rs` (`handle_fiber_resume_signal_jit`) |
| Resume-value handoff | `src/primitives/fibers.rs:127` (`prim_fiber_resume`), `src/vm/fiber/refcount.rs:159` (`release_parked_signal`) |
| Culprit test | `tests/elle/process-io.lisp` (tests 10–12) |
| Sibling (already fixed) | `tests/elle/region-jit-io-suspend-uaf.lisp` |
| Region model docs | `docs/impl/region/generations.md`, `effects.md`, `owner.md`, `rules.md` |
