# The judgment

<!-- audited: 2026-11-23 -->

How the language decides that a program is accepted: Datalog rules over the core forms, a verifier for the IR, and leek as the exact judge.

The verifier, leek and leek's generator are a design, and none is built.
[The memory model](memory.md) is what the judgment enforces, and
[the ownership rules](ownership.md) and [signatures](signature.md) are its
rules in words.

## Refusal is a rule of the language

The specification states the ownership rules as Datalog over facts of the core
forms, and the verifier decides acceptance by running them. Every implementation refuses the same programs,
and a refusal test is a language test.

The core forms are the kernel that [HIR](../impl/hir.md) lists. Every other
form is sugar over the kernel, and the specification states how each sugar
form becomes kernel forms.

The judgment is a mechanical decision procedure. Its rules are Datalog, and
they run on [proof](../stdlib/proof.md). The compiler proposes a lowering, and
the verifier decides whether to accept it. The compiler's own inference is not
the judgment, because that would use the compiler to judge the compiler.

The rules read facts from two sources:

| Source | What its facts decide |
|--------|-----------------------|
| The core forms | whether the language accepts the program |
| [The region IR](ir.md) | whether the lowering is valid |

A hand proof shows that an accepted program has a valid lowering.

## The verifier

The verifier checks each function against its claimed signature, and assumes
the claimed signatures of its callees. It makes three checks:

- Each region is freed once on every path.
- No use is reachable after its free.
- Each borrow ends before its owner does.

The verifier is written in Elle. It reads the IR as data, and it shares no
code with the compiler.

## Leek

Leek is the exact judge. It is a subcommand written in Elle. It evaluates the
IR under the specification's region semantics, holds its own frames, and
models the subject's regions as its own data. A use-after-free in the subject
is therefore a verdict and not a fault.

Leek holds two invariants:

- At each free, no live pointer reaches the region.
- A region that nothing reaches has been freed.

Leek cannot see a free that is late while a live slot still names the region.
The verifier owns that case.

Leek's corpus is property-based. A generator written in Elle, on
[check](../stdlib/check.md), enumerates every small program up to a size
bound, then samples past it. It calls leek and the verifier in-process. The
property has two halves: a lowering the verifier accepts draws no verdict from
leek, and a planted fault, a deleted free or an early free, draws one.

## Targets

A target is a tight rule the judgment enforces while its feasibility is
tracked. A program that fails a target is refused. A target becomes a plain
rule when no known program fails it, and giving one up is a change to the
specification.

| Target | Stated in |
|--------|-----------|
| The crossing of a fiber | [the ownership rules](ownership.md) and [crossing](crossing.md) |
| A call to a parameter | [signatures](signature.md) |
| The five instructions that store or allocate by a run-time tag | [the region IR](ir.md) |
| A value live after a join in two states | [release](release.md) and [states](states.md) |
| A value made again while the value made before is live | [again](again.md) |
| A parameter stored into two places | [signatures](signature.md) |

[The ratchet](../ratchet.md) pins a count once a ledger holds one. No ledger
holds a target's count yet.

## A language concern

The judgment is a language concern. It is not built against
[the rig](../../rig/overview.md), because a rig is built per backend.

## Not written yet

- The Datalog form of each rule that [datalog](datalog.md) and the documents it
  names do not hold yet.
- The translation of each sugar form into kernel forms.
- The hand proof.
