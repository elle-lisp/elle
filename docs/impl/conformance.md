# Conformance of the region system

<!-- audited: 2026-11-21 -->

Where this implementation differs from the memory model, the freeze that holds it still, and the steps to the flip.

[The memory model](../spec/memory.md) is the specification's, and
[the region memory model](memory.md) says what this implementation does. This
implementation does not conform.

## What differs

| The model | This implementation |
|-----------|---------------------|
| No class of region is counted | Every region is born counted, and an adoption is what removes its count ([ownership](region/ownership.md)) |
| An immutable value is not counted | The allocation scan counts each region an immutable object refers to ([the region rules](region/rules.md)) |
| A read borrows | A name bound to a read out of a reassigned binding takes a counted reference ([reads](region/reads.md)) |
| A fresh result is owned by the caller's fiber | A return hands the caller one counted reference to the result's region ([the mechanism](region/mechanism.md)) |
| A program the rules cannot place is refused | Every program is accepted, and a region the compiler cannot bound keeps a count |

## The freeze

From the day the specification lands, no change lands in the present region
inference, its lowering or [the region documents](region/AGENTS.md). A defect
gets an issue and a witness seed, as
[#1505](https://github.com/elle-lisp/elle/issues/1505) did. Main keeps each
known use-after-free until the flip. A fix that removes a leak and adds no
release may land, with its witness test.

## An observer migration

The new inference lowers each program a second time. The verifier and leek
judge that lowering while the backends run the present one
([the judgment](../spec/judgment.md)).

- A grow-only ledger records the accepted programs. The first accepted
  programs are bare, without the stdlib, and the stdlib enters the ledger as
  the classes it needs land.
- The flip comes when the ledger covers both suites.
- The two disciplines never mix in one program.
- Backends first run the new discipline at the flip. Before it, a backend
  refuses a function that holds an instruction of
  [the region IR](../spec/ir.md).
- The verifier is an observer until the flip. At the flip the boot image
  carries the compiled verifier, as it carries the stdlib.

Once the lowering step builds the refusal counts, the tree is born conforming.
A function new to the tree may not add a refusal in its own body, and a
changed function may not gain one. A call into unconverted stdlib does not
count.

## The steps, in order

1. **The specification,** with the engine it needs: one order for `compare`,
   [check](../stdlib/check.md) and [proof](../stdlib/proof.md).
2. **The region IR.** The new lowering emits it from the kernel forms. This
   step adds a desugaring pass from today's HIR to the kernel, and that pass
   feeds only the new lowering. The step also gives each native its
   signature, and hands the IR to Elle as structs.
3. **Leek,** with its enumerating generator. Its first tests are hand-built
   IR with a planted stale pointer and a planted unreached region, and it
   fails both.
4. **The verifier.** Its first tests are one fact set per rule that it
   accepts, one that it refuses, and hand-built IR it rejects.
5. **The uncounted lowering.** It takes the rules' facts from the core forms
   of real programs, bare programs first. This step builds the refusal counts
   per rule, pins them on [the ratchet](../ratchet.md), starts the
   born-conforming gate, and gives each target its count.
6. **Counted classes,** one at a time, in the order the refusals demand.
7. **The flip.** The pipeline emits the validated IR. The present lowering,
   the count protocol and [the oracle](../../tests/impl/oracle.lisp) are
   deleted, and leek stands where the oracle stood.
