# The audit queue

<!-- audited: 2026-09-05 -->

Every file carries the day it last met the documentation policy, and the queue
names what to read next by what a stale file costs.

## The stamp

A source file carries the stamp in its header block. A document carries it in
an HTML comment under the title.

```
// audited: 2026-09-04
```

The date is ISO 8601. A file with no stamp has never been audited.

A file that cannot meet a rule yet stamps `audited: <date> (#N)`, naming the
issue that will fix it. A deviation with no issue behind it is a broken rule,
and the file carries no stamp at all.

## Enforcement

`scripts/audit --staged` fails a commit whose staged files carry no stamp for
today. It runs from `.githooks/pre-commit`, beside the formatter.

The gate asks one question: you changed this file today, so did you read it and
hold it to the policy? Touching a file is the moment its cost is already paid —
the file is open, its subject is loaded, and the reader is the person best
placed to notice that a paragraph has gone wrong.

The gate cannot ask whether the audit was honest. It makes the claim explicit
and dated, which is enough to make a false claim visible later.

## What the queue is for

The gate keeps files fresh as they are touched. It reaches nothing else.

Untouched files are the whole problem. Garbage is by definition what nobody
opens, so the set the gate never sees is exactly the set that rots. The queue
is how that set is worked through on a cadence.

## Ordering by cost, not by age

The obvious queue sorts by stamp age, oldest first. That is the wrong key.

Recency of touch is close to anti-correlated with being wrong. A file stamped
last week was read last week. A file that has never been stamped is usually
just older than the policy, which says nothing about whether it misleads
anyone.

Sort by what a stale file costs instead. A document's cost is roughly its size
multiplied by how often it is read, and read frequency falls sharply with depth
below the entry point:

- A root document is read by every session, forever.
- A directory's index is read by sessions working in that directory.
- A leaf is read by a session already working on its subject.

So the queue ranks a file by size and shallowness together, with unstamped
files ahead of stamped ones at equal rank. A large root document with no stamp
comes first. A small leaf at depth four comes last, and may never come up at
all, which is the correct answer for a file whose staleness costs nothing.

## Commands

| Command | What it does |
|---------|-------------|
| `scripts/audit` | the ten costliest unaudited files, and the counts |
| `scripts/audit --next N` | the N files to audit next |
| `scripts/audit --stale DAYS` | every file stamped longer ago than DAYS |
| `scripts/audit --all` | every file, costliest first |
| `scripts/audit --staged` | fail when a staged file carries no stamp for today |
| `scripts/audit --policy` | every file stamped before the policy's own stamp |
| `scripts/audit --root DIR` | treat DIR as the repository root |

`make audit` runs the first form.

`--staged` with no paths reads the staged list from git. Given paths, it checks
those instead, which is what lets the gate be tested without staging anything.

`--root` exists so the queue can be ranked over a fixture tree. The ordering is
the part of this design most likely to be got wrong, and it cannot be pinned by
a test that has only the real repository to rank.

## The cost function

`cost = bytes ÷ (depth + 1)`, and the queue sorts by `cost × staleness`
descending. Staleness is days since the stamp; a file with no stamp takes a
staleness larger than any real age, so it outranks a stamped file of equal
cost.

Depth is the count of path separators. It stands in for read frequency, which
is the quantity that actually matters and which nothing measures directly.

## What carries a stamp

Sources a reader edits, and the documents they cite. Generated trees carry
none, and neither does a vendored dependency.

A directory below the root that carries its own `COPYRIGHT` or `LICENSE` file
is vendored, and nothing under it is queued. The licence is the signal because
the tree already states it. A list of vendored paths inside the script would be
a second copy of that fact, and it would go wrong the first time a dependency
moved. The root's own licence covers the repository and exempts nothing.

A generated file is exempt because nobody audits its content — its generator is
the thing that gets audited. An `AGENTS.md` is exempt when it carries the
marker [the generator](agents.md) writes, and queued when it does not.

Exempting the name rather than the marker would drop every hand-written index
out of the queue, including the root one, which is the most-read document in
the repository and the costliest place for a stale claim to sit.

The eligibility test lives in one function, and the staged gate and the tree
walk both call it. Two copies would disagree about what is exempt, and the
disagreement would show up as a commit that the gate passed and the queue
reports as unaudited forever.

## The policy's own stamp is the floor

[The policy](../../DOCUMENTATION.md) carries a stamp like any other document,
and changing a rule stamps it with that day. `--policy` lists every file
stamped before it, which is the count of what a policy change has not reached.

A policy change therefore returns most of the tree to the queue at once. That
is why the queue is an order and not a work list to finish: the cost ranking
brings back the files where a stale rule is read most often, and leaves the
rest at the bottom.

## A stamp is a claim the build can falsify

The stamp attests to three things a machine cannot decide. Everything else in
the policy is checkable, and `tests/integration/prose.rs` checks it.

Those checks run **only against files that carry a stamp**. An unstamped file
has claimed nothing and is exempt; a stamped one has claimed to meet the
policy, so a violation in it is a false claim and fails the build. Coverage
grows as the tree comes into policy, and it needs no flag day.

This is the hedge against a stamp applied without an audit. Confirming the line
count no longer discharges the claim, because the line count was never the
reader's job. What is left to attest is the part that takes reading the file:
whether it is still true, whether it earns its length, and whether it holds
something that belongs elsewhere.

A stamp can still be false — an agent can read nothing and stamp a file that
happens to pass every check. The checks remove the failures that look like
diligence, so what remains is a claim somebody made about content, which a
later reader can find wrong and act on.

## Repair is the point

A stamp records that a file met the policy on a day. Applying the policy to a
file that violates it means fixing the violation, not recording the reading.

So an audit that finds a stale paragraph rewrites it, in the same change. A
fresh stamp over a standing violation is a false claim, and it is worse than no
stamp: it removes the file from the queue that would have brought somebody back
to it.
