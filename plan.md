# Plan: release unbound Call result regions

## Context

The unique-region rewrite (steps 1–13) and the previously-approved return-as-escape mechanism (step 14) are landed. What's currently in the tree:

- `ReleaseValueRegion(reg)` LIR/bytecode/dispatch opcode that reads a value's runtime region via `region_of()` and decrefs it.
- `RegionInfo.lambda_tail_regions` populated by the walk; the lowerer's `emit_decrefs_for` suppresses `DecrefRegion` for any region that flows out of the currently-active lambda's body as a tail return value.
- The top-level entry function is registered as an implicit lambda for the same suppression.
- For let-bound Call results, `lower_let` records `call_region_slot[call_r] = binding_slot`, and `emit_decrefs_for` emits `LoadLocal binding_slot` + `ReleaseValueRegion` at the call's `free_at`.

The remaining gap is unbound Call results: `(use (foo))`, `(emit :yield (foo))`, `(push acc (foo))`, etc. In these shapes, `foo`'s result flows directly into the consumer as an arg or stack value. There is no binding to anchor a slot. The current `emit_decrefs_for` falls through to a `continue` and emits nothing — the region's RC stays at 1 forever, surviving until fiber teardown. That's a leak, and it's not acceptable.

This plan removes that fallback by making every Call (bound or not) carry its own release slot.

## Approach

In `lower_call`, after emitting `LirInstr::Call { dst, ... }`:

1. Allocate a fresh local slot — `release_slot` — for this Call.
2. Emit `LirInstr::StoreLocal { slot: release_slot, src: dst }` to stash the result.
3. Emit `LirInstr::LoadLocal { dst: reload_reg, slot: release_slot }` to put the same value back on the operand stack for the parent consumer.
4. Record `call_region_slot[call_r] = release_slot`.
5. Return `reload_reg` from `lower_call`.

Then `emit_decrefs_for(hir_id)` already does the right thing — for any region in `call_result_regions`, it looks up `call_region_slot`, emits `LoadLocal slot` + `ReleaseValueRegion(reg)`. This works uniformly for bound and unbound Calls now that every Call has a slot.

Delete the special-case slot recording in `src/lir/lower/binding.rs::lower_let` — it's redundant once `lower_call` owns the slot.

## What happens to the slot for tail-region Calls?

This is the question the user raised, and the previous plan handled it badly. The answer: **the slot is allocated, the value is stashed, and `emit_decrefs_for` suppresses the Release for tail regions. The region is not leaked — its initial RC=1 is transferred to the caller.**

Concretely, for a function whose tail expression is a Call:

```
(fn () (foo))   ; foo's call_r is in lambda_tail_regions[lambda.id]
```

- Lowerer emits `Call foo`, then `StoreLocal release_slot`, then `LoadLocal reload_reg`.
- `emit_decrefs_for` at the Call's `free_at` sees `call_r ∈ lambda_tail_regions` → skips. **No Release in the callee.**
- `Return reload_reg`. The value flows out of this frame.
- The callee's frame (and its slot) is discarded by the VM at frame teardown. The slot was a stack-local indirection; the heap region the value points into is independent.
- The **caller's** `lower_call` for whatever invoked this lambda allocated *its own* `release_slot_outer` and stashed the returned value there. The caller's `emit_decrefs_for` at the caller's call's `free_at` emits `LoadLocal release_slot_outer` + `ReleaseValueRegion` — that's the decref that brings RC=1→0.

The chain bottoms out at the outermost caller (the runtime or test harness), whose `vm.call_closure` returns the value to Rust. The value's region is then either dropped via the harness's normal teardown (test ends, VM is dropped, all regions freed) or explicitly released by `with_transient_region!` macros already used in `vm/call.rs`. No leak.

So the slot for a tail Call is allocated but not read by the callee's own bytecode at runtime. That's not "dead code" — it just means the Release responsibility lives one frame up. The slot allocator could in principle elide tail-position release_slots (since they'll never be read inside this function), but it's not a correctness requirement and the simpler "always allocate" rule is fine for the unmerged baseline.

## Composition

- `(foo (bar))`: `bar`'s Call allocates `release_slot_bar`. The lowerer emits the stash/reload pair. `foo`'s Call also allocates `release_slot_foo` for the outer call's result. At `bar`'s `free_at` (which is at `foo`'s call hir_id under normal last_use), Release fires from `release_slot_bar`, decrementing bar's actual runtime region. At `foo`'s `free_at`, Release fires from `release_slot_foo`, decrementing foo's actual runtime region. Both regions are released; nothing leaks.

- `(let [y (bar)] (use y))`: `bar`'s Call allocates `release_slot_bar`. `lower_let` then `StoreLocal binding_slot, reload_reg` to bind `y` (a separate slot). At `bar`'s `free_at = Call(use).id`, Release fires from `release_slot_bar` (not from `binding_slot` — that path is now redundant). Then `use`'s Call follows the same pattern with its own `release_slot_use`.

- `(if cond (foo) (bar))`: both branches allocate their own `release_slot_*`. The if's result reg is the merge of both branches' `reload_reg`s. `emit_decrefs_for` at the if's `free_at` fires Release from whichever branch's slot was set — actually both branches have their own slots, and `emit_decrefs_for` iterates over all regions demising at this hir_id, emitting a Release for each. RC=0 for whichever branch ran; the other branch's region was never alloc'd at runtime (the branch wasn't taken), so its Release on an absent region is a no-op (already the behavior of `decref_region` on a region with no allocs).

## Files to modify

- `src/lir/lower/control.rs::lower_call` — insert the stash/reload pair and slot recording after `LirInstr::Call` emission.
- `src/lir/lower/binding.rs::lower_let` — remove the now-redundant `call_region_slot` recording.

That's the entire change. The dispatch handler, `ReleaseValueRegion` opcode, `RegionInfo` fields, lambda-stack tracking, tail suppression, and `emit_decrefs_for`'s branching are all already in place.

## Critical existing utilities to reuse

- `crate::value::arena::region_of(value)` at `src/value/arena.rs:75` — runtime region lookup; already wired into the dispatch handler.
- `FiberHeap::decref_region(id)` at `src/value/fiberheap/mod.rs:164` — the actual decref.
- `Lowerer::emit_decrefs_for` at `src/lir/lower/mod.rs:430` — the single emission hook; already does the right thing once every Call has a slot recorded.

## Verification

`make test` (which depends on `make smoke`) is the gate:

1. `cargo build --bin elle` succeeds.
2. `make smoke` reaches the end. The currently-failing `callable-resume.lisp`, `arena.lisp`, and the broader VM-only pass all proceed past their previous failure point. (If any test asserts old scope-retention semantics in a way that the new model genuinely doesn't satisfy — e.g. `arena.lisp`'s unused-`_` allocation count — surface that as a test-side change for separate user approval rather than silently rewriting.)
3. `make test` reaches the end.
4. `./target/debug/elle --trace=rc <file>` for `(foo (bar))`-shaped programs shows balanced `[trace:rc] ReleaseValueRegion(N)` lines at both inner and outer call free_at points. For `(emit :yield (string "..."))`, the trace shows `[trace:rc] emit incref(N)` at the Emit handler and the matching Release at the resume site or the consumer of the yielded value.
