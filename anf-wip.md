# ANF work-in-progress notes

Companion to `anf-plan.md`. Records what's landed, what's deferred, what
this session learned (with reproducers), and the open questions for the
next session.

## What's landed in this commit (Step 1 only)

- `src/hir/anf.rs` — module skeleton. `anf_lift(&mut Hir, &mut
  BindingArena)` is a no-op for now. Doc-comment cites `anf-plan.md`
  for the design.
- `src/hir/mod.rs` — `pub mod anf;`.
- Pipeline wiring inserts `crate::hir::anf::anf_lift(&mut hir, &mut
  arena)` immediately after `functionalize` in every callsite the plan
  named:
  - `src/pipeline/compile.rs:64,219,355`
  - `src/pipeline/eval.rs:52,116`
  - `src/pipeline/cache.rs:126`
  - `src/vm/eval.rs:153`
  - Test helpers: `src/lir/lower/mod.rs:692`, `src/hir/liveness.rs:661,766`,
    `src/hir/regions.rs:1262,2377`, `src/hir/defuse.rs:383`
- Bonus, requested mid-session:
  - `src/config.rs` — `trace_bits::ANF = 1 << 19`, `from_name("anf")`,
    `ALL` widened accordingly. Bit is reserved; nothing emits yet.
  - `src/main.rs` — `--help` now lists `rc`, `regions`, `anf` under
    `--trace=KW` and shows `--no-stdlib` / `--no-uring` flags. The
    `--no-uring` description was corrected to "Linux: disable
    io_uring; route I/O through the thread pool" — there is no
    blocking I/O in Elle.

State at HEAD: 1127 lib tests pass, 2 pre-existing failures
(`lsp::state::test_document_open_and_close`,
`test_document_change`). Both pass when run in isolation — parallel
test isolation issue, not caused by this work. Smoke runs cleanly
(`elle /tmp/x.lisp` prints `3`).

## What's deferred and why

Steps 2 through 7 of `anf-plan.md` are NOT in this commit. The plan
claims "smoke green at each step"; in practice Step 2 alone breaks
smoke because the wrap of ANF synthetic bindings interacts with the
existing shadow `call_region_slot` mechanism. They needed to land
together. Even Steps 2+3+4+5 + a liveness propagation fix +
amendment to the `allocates` predicate left one unresolved
regression (see below). Reverting to Step 1 only is the safe, green
floor we ship.

## Findings that should inform Step 2's redesign

### Finding 1: the plan's `Hir::allocates` is wrong about DerefCell

`anf-plan.md` lists `DerefCell{..}` as allocating. Wrapping it broke
core.lisp's compile: the exports closure came back with NIL for every
captured binding.

Why: `src/lir/lower/binding.rs::lower_deref_cell` is *transparent* — it
delegates straight to `lower_expr(cell)`, and `lower_var` auto-unwraps
needs-capture bindings via `LoadCapture`. There is no real heap alloc
at the DerefCell node. Region inference at
`src/hir/regions.rs:501-507` calls `alloc_here(hir.id)` anyway,
treating it as a *phantom* region — "opaque, treat as a fresh
allocation; the runtime tracks the actual referent." So the region
exists at compile time but corresponds to no runtime allocation.

When ANF wraps `(deref-cell x)` → `(let [t (deref-cell x)] t)`:
- `t`'s binding gets that phantom region in `binding_regions[t]`.
- Chain extension at `src/hir/regions.rs:1140` extends the phantom
  region's `free_at` to wherever `t` is consumed.
- `emit_decrefs_for` at that `free_at` emits `DecrefRegion(phantom)`.
- At runtime, decref'ing a region that holds zero objects either
  goes negative or double-counts neighbouring regions.

Empirically: removing DerefCell from `Hir::allocates()` is what made
core.lisp's exports come back as proper closures instead of all-nil.

**Action for Step 2**: drop DerefCell from `Hir::allocates()`. Add a
comment that ties it to `regions.rs:501-507`. Likely also worth
auditing `MakeCell` and `Eval` against the same "phantom region vs.
real allocation" question before keeping them in the predicate.

### Finding 2: `liveness::walk` needs to propagate `parent_consumes` through Let body

ANF puts every allocating expression at a Let init position, producing
the pattern `(consumer (let [t e] t))`. The existing `walk` in
`src/hir/liveness.rs` walks Let body with `parent_consumes=false`,
hard-coded:

```rust
HirKind::Let { bindings, body } => {
    for (b, init) in bindings { ... self.walk(init, true, hir.id); }
    self.walk(body, false, hir.id);   // <-- always false
}
```

The comment says "body propagates (its value is the form's result)" —
but the propagation is only one level. `Var(t)` inside the body ends
up with `last_use = Var(t).id`, the chain extension at
`regions.rs:1140` therefore stops at `Var(t).id`, and the call's
region's `free_at` is the synthetic Var node *inside the Let* — i.e.
before the outer consumer reads `t`'s value. Result: release fires
before consume, use-after-free at the outer consumer.

The fix that worked in-session:

```rust
HirKind::Let { bindings, body } => {
    for (b, init) in bindings { ... self.walk(init, true, hir.id); }
    self.walk(body, parent_consumes, parent_id);
}
```

Same for `Letrec` and `Loop`. With this change, `Var(t).last_use`
becomes the outer consumer when the Let is itself in a consuming
position, and the chain extension correctly extends the call's region
through to the consumer.

**Open question for next session**: is this actually correct in all
cases, or just for ANF-introduced lets where the body is `Var(b)` for
the binding `b`?
- For a user-written `(g (let [x foo] (do-something x)))`, the Let
  body isn't a bare Var — it's a Call. The body's value (the Call's
  result) flows out. With the propagation, `last_use[body.id]` becomes
  the outer `g`'s id; without it, it's the body's own id. Does the
  binding chain extension then over-extend the Call's region?
- More importantly, the propagation also affects `Letrec` and `Loop`,
  which always have a body that's an arbitrary expression. The change
  was untested for those cases beyond "stdlib still compiled with the
  change in place when DerefCell was excluded from `allocates`".
- Possible refinement: only propagate when the body is literally
  `Var(b)` for one of the let's bindings — i.e. recognize the ANF
  wrap pattern explicitly. That keeps the existing semantics for
  user-written lets and only changes behavior for shapes ANF
  introduces.

Alternative angle: maybe the right fix isn't in `walk` at all but in
the chain-extension at `regions.rs:1140`, which already exists
specifically to handle "value flows through a binding". It could
walk the binding's body through any chain of non-consuming wrappers
to find the real consumer. That'd avoid touching liveness at all.

### Finding 3: Steps 2 + 4 + 5 must commit together

The plan calls for "Step 2 (ANF wraps) + Step 4 (region_to_slot) +
Step 5 (delete shadow `call_region_slot`)" as three separate commits,
with the claim that Step 2 alone keeps smoke green because "the old
release path is still active."

That claim is false on this branch. With Step 2 + the old shadow
mechanism, `wrap_call_with_release_slot` (`src/lir/lower/mod.rs:446`)
allocates a release slot for every non-tail Call, *and* the ANF
wrap's `(let [t (call)] t)` independently allocates a binding slot.
`emit_decrefs_for` then fires `ReleaseValueRegion` via the shadow slot
at the binding's `free_at`. Two slots hold the same value; the
shadow's release path uses one, the value is still live via the other,
and the timing mismatch produces premature decref-region storms at
letrec exit that clobber capture cells (this is the bug that made
core.lisp's exports come back as nil before Finding 1 was applied).

**Action**: land Steps 2 + 4 + 5 in a single commit. The plan's
ordering is wrong about commit boundaries.

### Finding 4: an unsolved bug — stdlib.lisp's `+` and `-`

With all of Steps 2 + 3 + 4 + 5 + the liveness propagation fix +
DerefCell removed from `allocates`, core.lisp compiles cleanly and
its exports return as proper closures. But stdlib.lisp's export
struct comes back with `+` and `-` as nil, while every other entry
(including `*`, `/`, `<`, `>`, all the predicates, all the streams,
parameters, etc. — many structurally identical to `+`) is correct.

Reproducer (with Steps 2+3+4+5 applied):
```
echo '(println (+ 1 2))' > /tmp/x.lisp
target/debug/elle /tmp/x.lisp
```
Panics:
```
stdlib export '+' is neither closure nor parameter: nil
```

What I tried (and ruled out):
- Not wrapping Letrec body → same failure.
- Not wrapping If branches → same failure.
- Both `+` (line 314) and `-` (line 327) of `stdlib.lisp` come back
  nil; `*` (line 345, structurally identical to `+`) works.
- The user's primitive table does NOT pre-register `+` or `-`. They're
  pure stdlib definitions.
- Source-order doesn't seem to be the discriminator (`+` is defined
  before `-` and `*`).

What I didn't try:
- Inspecting the LIR of the export closure specifically — see whether
  `+`'s capture index differs from `*`'s.
- Adding traces inside `lower_letrec`'s init loop to record what
  `StoreCaptureCell` writes for each binding.
- Inspecting whether `+` 's letrec binding's signal or arity metadata
  is malformed by ANF (which could trip later code that expects a
  closure).
- `--dump=regions` on stdlib — but stdlib is huge and triggers cache
  init before the user program's `--dump` can fire. Would need to
  temporarily redirect `--dump` into compile_core.

**Where to start in the next session**: turn on `--trace=anf --trace=rc`
and find the exact `decref` (or absence of `alloc`/`StoreCaptureCell`)
for the `+` binding's closure during stdlib compile_file. The `+`
binding's cell either (a) never received its closure via
`StoreCaptureCell` or (b) had its closure region freed before the
export closure ran. Trace will discriminate.

## Tooling added this session

### `--trace=anf`

`trace_bits::ANF = 1 << 19`. Reserved bit, no emission sites yet.
When the actual transform lands, each `wrap_value` call should emit:

```
[trace:anf] wrap <kind> @<HirId> (span=<line>:<col>)
```

Modelled on the existing `[trace:rc]` and `[trace:regions]`
formatting. Will let you bisect "which wrap broke X" without env
vars.

### `--no-stdlib` and `--no-uring` in `--help`

`--no-stdlib` was already a CLI flag but missing from `--help`.
Essential for debugging compile_core / prelude in isolation:
short-circuits `init_stdlib` so the compilation cache still runs
(prelude + core.lisp) but stdlib.lisp isn't loaded. Lets you test
small programs without stdlib dependencies (and without macros that
require stdlib).

`--no-uring` was also missing. Linux-only; routes I/O through the
thread pool instead of io_uring.

## Useful diagnostic recipes from this session

### See which closure is captured wrong

`--trace=rc` emits `alloc_in_region`, `incref`, `decref`,
`ReleaseValueRegion`, and `FREE objs=N` events. The pattern to look
for when a capture goes nil is:
```
alloc_in_region(R) tag=Closure count=N    <-- closure created
... [no later StoreCaptureCell to its cell] ...    OR
... decref(R) → rc=0 → FREE ...    <-- closure freed
```

### See region inference output for stdlib

`--trace=regions` dumps `format_regions` after every compilation.
Compile_core's output appears early in the trace; the user file's
output is at the end.

### Bisect which ANF position breaks things

With `--trace=anf` (once the wrap fires), grep the output to count
wraps per file and per kind. The plan's allocating positions can
be selectively disabled in `anf_lift`'s `wrap_value` for
bisection.

## Files to touch when resuming

Per the plan, with my notes appended:

- `src/hir/anf.rs` — replace no-op with the actual transform. Tests
  modules from the session (9 unit tests for positions 1-9) can be
  resurrected from git reflog or rewritten from the plan.
- `src/hir/expr.rs` — add `Hir::allocates()` *without* DerefCell.
- `src/hir/pattern.rs` — add `pattern_allocates()`.
- `src/hir/mod.rs` — re-export `pattern_allocates`.
- `src/hir/liveness.rs` — apply the propagation fix (or the
  alternative chain-extension fix in `regions.rs:1140`) after
  deciding which is correct in Finding 2's discussion.
- `src/lir/lower/mod.rs` — rename `call_region_slot` →
  `region_to_slot`; delete `wrap_call_with_release_slot`; rewrite
  `emit_decrefs_for` to use the new field.
- `src/lir/lower/binding.rs` — populate `region_to_slot` in
  `lower_let` and `lower_letrec` after `allocate_slot`, keyed on
  `region_info.alloc_region[init.id]`.
- `src/lir/lower/control.rs:85,170` — replace
  `wrap_call_with_release_slot(dst)` with `Ok(dst)`.
- `src/lir/lower/lambda.rs` — save/restore `region_to_slot` across
  the lambda boundary (the same pattern as `region_to_table`).
- `src/lir/lower/mod.rs::body_is_tail_call` (Step 3) — add the
  `(let [b e] (var b))` case so tail-call recognition survives ANF.
- `src/hir/anf.rs` `wrap_value` — emit `[trace:anf]` events behind
  the trace bit added in this commit.

## Why the session didn't finish

Two compounding reasons. First, the plan's design has at least three
errors that only show up at runtime, each of which costs hours to
discover: DerefCell as phantom region (Finding 1), liveness needing
propagation (Finding 2), and Step 2 not being smoke-safe alone
(Finding 3). The unit tests written from the plan all pass at the
HIR level — they validate the *shape* of the rewrite, not the
runtime consequences.

Second, I spent too long ad-hoc `eprintln!`-debugging before
recognizing that `--trace=rc` already prints exactly what I needed,
and that `--no-stdlib` would have given me clean reproductions much
earlier. The user pointed both out explicitly mid-session. Lesson:
read `--help` and grep the trace infrastructure *before* adding new
prints.

The remaining `+`/`-` bug (Finding 4) is the last shoe to drop and
the right starting point for the next session.
