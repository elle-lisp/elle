# A direct self-call

<!-- audited: 2026-11-23 -->

A compiled function that calls itself with immediate arguments calls its own native code, with no dispatch helper.

Every other call a compiled function makes goes through `elle_jit_call`
([jit.md](jit.md)). For a self-recursive function such as `fib` that helper is
most of the cost of a call: it resolves the callee, looks up its code, checks
its arity, hands each owned parameter a reference, and builds the callee's
boundary when a signal returns. When the callee is the running function and
every argument is an immediate, all of that is known when the function
compiles, or does nothing.

## What is admitted

The translator compiles a non-tail call to a direct call when all of these
hold at compile time. Otherwise the call goes through `elle_jit_call`.

| Condition | What it settles |
|-----------|-----------------|
| A `LoadSelf` defines the callee register | The callee is the running closure. Its code is this code, and its environment is the one this frame received |
| The arity is `Exact(n)`, and the call passes `n` arguments | The arity check, made once at compile time |
| The call offers no hand-off | No argument moves into the callee, so no word reports what it took ([handoff.md](region/handoff.md)) |
| Each argument is a constant immediate, or the result of an operation that carries `OperandProof::Int` | An immediate lives in no region, so the reference a call hands each owned parameter is a no-op |

A tail call to the running closure is a loop instead ([jit.md](jit.md)).

## The guards

Three conditions are facts of the run, not of the compile. The direct call
reads each before it calls. When any one fails, the call goes through
`elle_jit_call`, which runs it as it runs every other call.

| Guard | Reads | Fails when |
|-------|-------|------------|
| The squelch mask is empty | `Closure::squelch_mask`, through the self payload | `squelch` or `attune` built the running closure |
| The native stack has room | the stack pointer, and `VM::stack_floor` | less than 512 KiB of stack remain ([vm.md](vm.md)) |
| The depth cap has room | `Fiber::call_depth`, and `RuntimeConfig::max_depth` | the call would pass `(vm/config :max-depth)` |

The stack guard makes the compare `StackFloor::below` makes, as a wrapping
subtraction, so a floor of 0 and a stack the floor does not describe never
read low. The direct call adds one to the call depth before it calls, and
takes one away when the callee returns, as `elle_jit_call` does.

## The boundary it skips

`elle_jit_call` checks the signal that leaves a callee against the callee's
boundary: its squelch mask, and the proof a silent function carries
([signals and the JIT](../signals/jit.md)). A direct self-call checks neither, and the
answer is the same.

- The squelch guard admits only a closure with an empty mask, and an empty
  mask refuses nothing.
- A function that is not silent carries no proof, so its boundary does nothing.
- A silent function's proof names its code object, which the caller shares.
  A signal that breaks it leaves the callee and reaches the caller's check.
  A silent function records no park site, so the caller returns at once, and
  the boundary of the call that entered the caller raises the same
  `:broken-proof` error. No frame parks between the two boundaries.

## A tail call out of the callee

A callee that ends in a tail call to another closure returns
`TAIL_CALL_SENTINEL`, and leaves the call pending on the VM. The direct call
tests the result for the sentinel, and on it calls `elle_jit_self_call_tail`.
That helper runs the pending call as the sentinel arm of `elle_jit_call` runs
it: inside the boundaries of both the tail callee and the running closure.
The parameter depth it reads is the depth at the call, because a
`parameterize` body is never in tail position.

## Where it lives

| File | Holds |
|------|-------|
| [selfcall.rs](../../src/jit/translate/selfcall.rs) | Admission, the guards, and the call |
| [sentinel.rs](../../src/jit/calls/callops/sentinel.rs) | The pending tail call, run for `elle_jit_call` and for a direct self-call |
| [selfcall.rs](../../src/jit/compiler/tests/selfcall.rs) | Which calls are admitted |
| [jit-selfcall-tail.lisp](../../tests/impl/jit-selfcall-tail.lisp) | The sentinel |
| [jit-selfcall-squelch.lisp](../../tests/impl/jit-selfcall-squelch.lisp) | The squelch guard |
| [jit-selfcall-cost.lisp](../../tests/impl/jit-selfcall-cost.lisp) | The cost, against a self tail loop |
| [deep_recursion.rs](../../rig/tests/deep_recursion.rs) | The stack guard and the depth guard |
