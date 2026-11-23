# An unused parameter, in Datalog

<!-- audited: 2026-11-23 -->

How a value handed to a block parameter that no later step uses is released after the jump, as Datalog that runs on proof.

[Release](release.md) holds the Unused parameter clause, and
[the ownership rules](ownership.md) state the rule in words.
[Recur](recur.md) names the jumps that hand a value to a block parameter.

A jump ends the hold of the value it hands over, under the Recur rule. The
Unheld value rule releases a value only on an edge out of a step where the
value is live or made, and a jump does neither for its parameter. So without
this rule, a parameter that no later step uses has no release, and an `if`
whose value nothing reads keeps the value of each arm.

The clause releases only a value the fiber holds. A value read out of a
container keeps the container as its owner when a jump hands it on, so the
release belongs to that owner. A sink, an owner not passed and a crossing each
hand a value to `[:sink i p]`, `[:owner x p]` or `[:cross p]`. Another
instance or fiber takes that hold and releases the value, so the clause skips
those names.

In `up`, each arm makes a value and hands it to `r`, and nothing uses `r`. So
`r` is released on the edge out of each jump. In `ub`, each arm reads a value
out of `m` and hands it to `r`. The value stays `m`'s, so nothing is released.
