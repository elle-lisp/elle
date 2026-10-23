# JIT fuel

<!-- audited: 2026-10-22 -->

How compiled code charges a fiber's fuel, reads the kill word, and parks where the budget runs out.

Compiled code charges a fiber's fuel ([runtime.md](../runtime.md)) at the
instructions where the interpreter charges it: one unit at each call, spliced
call, tail call and spliced tail call, and one at each backward jump. A self
tail call is a tail call, so the native loop it compiles to charges once per
trip. A program therefore spends the same budget on either tier, and a driver
that steps a fiber one unit at a time pauses it at the same instructions.

**The charge is inline.** The budget is one 64-bit word on the fiber, and
compiled code reaches it through the VM pointer. In a function that is not
silent, the charge loads the word, pauses on zero, and otherwise stores the
word less one. An unmetered fiber holds a word above every budget, 2^63 + 2^62,
so the charge needs no separate test for whether the fiber is metered. A budget
is a non-negative integer, at most 2^63 − 1, and a run needs 2^62 charges to
count the unmetered word down to that range. `fiber/fuel` answers `nil` for
any word above it. The interpreter charges in the same steps.

**Every charge reads the kill word first.** The VM holds a pointer to its
kill word ([runtime.md](../runtime.md)), which `sys/kill` writes from another
thread. The charge loads the pointer, loads the word through it, compares the
budget against it unsigned, and branches to a cold block when the budget is
below it. The cold block calls `elle_jit_kill`, which sets the fiber's signal
to the terminal `:killed` error, and returns `nil` from the function. The
caller's check after the call finds the signal, so the kill leaves compiled
code as a primitive's error does. The block spills nothing and parks no frame:
a terminal signal is never resumed, and the killed worker's heap is freed
whole. The check runs in a silent function as in any other, so a silent charge
is the check, then a branch on zero around the decrement.

**A silent function never pauses.** Its charge loads the word and stores it
less one unless it is zero, and nothing branches out of the function
([inference.md](../signals/inference.md)). A silent function records no
charge site, as it records no call site.

**A pause parks at the charge site.** On zero, compiled code spills its frame
and calls `elle_jit_fuel_pause`. The helper parks the frame at the bytecode
offset where the charging instruction starts, with the operand stack the
interpreter has there: a call's arguments and callee are still on it, and a
backward jump's stack is the one its edge trimmed to. The signal is `:fuel`
with a `nil` payload, and the park takes no resume value. On resume the
interpreter runs that instruction again, charges again, and continues.

The emitter records each charge site's offset and stack as a `ChargeSiteInfo`,
in the order the translator meets the sites, in every function that may
signal. A count mismatch rejects the compile, as it does for yield points and
call sites. A site in bytecode that only the interpreter runs is counted with
no charge: the jump an `emit` resumes through, and whatever follows a spliced
tail call.

**The paused frame resumes in the interpreter.** The frame runs to its end
there, a self-tail-call loop included. Its next non-tail call reaches compiled
code again through `try_jit_call`.

## Files

| File | Holds |
|------|-------|
| [src/jit/translate/fuel.rs](../../src/jit/translate/fuel.rs) | The inline charge, the kill check and the silent charge |
| [src/jit/translate/fuel/sites.rs](../../src/jit/translate/fuel/sites.rs) | The charge-site counter |
| [src/jit/suspend/charge.rs](../../src/jit/suspend/charge.rs) | `elle_jit_fuel_pause`, the park at a charge site, and `elle_jit_kill`, the kill's cold path |

## See also

- [impl/jit.md](jit.md) — the JIT: how a call and a signal leave compiled code
- [runtime.md](../runtime.md) — fuel budgets and the kill word
- [signals/jit.md](../signals/jit.md) — signals at a compiled call
