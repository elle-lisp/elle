# The audit queue

<!-- audited: 2026-10-22 -->

Every file carries the day it last met the documentation policy, and the queue
names what to read next by what a stale file costs.

## The stamp

A source file carries the stamp in its header block. A document carries it in
an HTML comment under the title.

```
// audited: 2026-09-04
```

The date is ISO 8601. A file with no stamp has never been audited.

A file that breaks a rule with no issue behind it carries no stamp. A file that
cannot meet a rule yet carries a deviation stamp, which names the issue that
tracks the rule it breaks:

```
// audited: 2026-10-19 (#1400)
```

The queue reads the date of either form the same way.

## Enforcement

`scripts/audit --staged` fails a commit whose staged files carry no stamp for
today. It runs from `.githooks/pre-commit`, beside the formatter.

The gate asks one question: you changed this file today, so did you read it and
hold it to the policy? Touching a file is the moment its cost is already paid.
The file is open, its subject is loaded, and the reader is the person best
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
| `scripts/audit --policy` | every file stamped before the policy date |
| `scripts/audit --counts` | the unstamped files, the files stamped before the policy date, and the files stamped with a deviation, as three counts |
| `scripts/audit --sources` | every queued file that is not a document, one path per line |
| `scripts/audit --documents` | every queued document, one path per line |
| `scripts/audit --root DIR` | treat DIR as the repository root |

`make audit` runs the first form, and `make qa` runs `make audit` before its
other checks. The report and `--next` show the head of the ranked queue and
exit 0 however long the queue is. A reader that stops early must not fail the
script, because a failing report fails `make qa`.

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

The walk lists the files git tracks when the root is the top of a work tree,
and every file under the root otherwise. A file git does not track is nobody's
to audit: a hand-off note or a scratch file would sit in the queue, and the
counts would differ between two checkouts of one commit. A fixture tree that is
no repository still has every file walked.

A directory below the root that carries its own `COPYRIGHT` or `LICENSE` file
is vendored, and nothing under it is queued. The licence is the signal because
the tree already states it. A list of vendored paths inside the script would be
a second copy of that fact, and it would go wrong the first time a dependency
moved. The root's own licence covers the repository and exempts nothing.

A generated file is exempt because nobody audits its content — its generator is
the thing that gets audited. An `AGENTS.md` is exempt when it carries the
marker [the generator](agents-index.md) writes, and queued when it does not.

[The Makefile](../../Makefile), [the standard library](../../src/stdlib.lisp)
and [the LIR instruction enum](../../src/lir/code/instr.rs) are exempt, and
they are the only repository files exempted by name. Everything else in the
queue is read whole by somebody: a document off an index, a source file off a
call site. Nobody reads these three that way:

- `make help` is how a reader finds an action, and the `##` line beside a
  target is what they read.
- `(doc name)` is how a reader finds a standard-library function, and the
  docstring is what they read.
- A match arm or a grep is how a reader finds an LIR instruction, and that
  variant's doc comment is what they read.

A reading budget over hundreds of lines of recipe, thousands of lines of
definitions, or nearly ninety variants of one enum measures a distance no reader
travels. So none of the three is on the size ledger either: the size producer
reads the queue's own lists ([the tool producers](../../tests/ratchet/overview.md)).
The enum cannot be split without nesting variants into a sub-enum, which
rewrites every match over it ([src/lir/AGENTS.md](../../src/lir/AGENTS.md)).

The exemption names the standard library's path, never its extension. Every
other `.lisp` file stays queued: [the core](../../src/core.lisp), [the
prelude](../../src/prelude.lisp), and every module under `lib/`.

Exempting the name rather than the marker would drop every hand-written index
out of the queue. Such an index holds module knowledge somebody wrote, and a
stale claim in it misleads every session working in that directory.

The eligibility test lives in one function, and the staged gate, the tree walk,
`--sources` and `--documents` all call it. Two copies would disagree about what
is exempt, and the disagreement would show up as a commit that the gate passed
and the queue reports as unaudited forever.

`--sources` prints the queued files that are not documents, one path per line,
sorted, and `--documents` prints the queued `.md` files the same way. Neither
reads a stamp, so a file with none is listed like any other.

## The policy date is the floor

[The policy](../../DOCUMENTATION.md) carries a second line under its stamp:

```
<!-- policy: 2026-09-04 -->
```

That date is the policy date: the day a rule last changed. `--policy` lists
every file stamped before it, which is the count of what a rule change has not
reached.

A rule change therefore returns most of the tree to the queue at once, so the
queue is an order and not a work list to finish. The cost ranking brings back
the files where a stale rule is read most often, and leaves the rest at the
bottom.

The stamp cannot serve as the floor. The policy is audited like any other file,
so an edit that fixes a link or corrects a sentence stamps it today. A floor
read off that stamp returns to the queue every file stamped since the last rule
change, though no rule changed. The script refuses a policy with no policy date,
because a floor it guessed would be wrong silently.

## The counts are on the ratchet

`scripts/audit --counts` prints three lines: `unstamped N`, `before-policy N`
and `deviation N`. The first counts the files with no stamp, the second the
plain stamps older than the policy date, and the third the deviation stamps of
any date. No file is in two of them.
[tests/ratchet/audit.lisp](../../tests/ratchet/audit.lisp) reads each count,
and [its ledger](../../tests/ledger/audit.lisp) pins each one
([the ratchet](../ratchet.md)).

The walk starts the same number of processes however many files the tree
holds. The producer runs under the runner's per-file deadline, once per JIT
policy and both at once, and starting a process costs milliseconds on macOS.
So the eligibility test and the stamp read use shell builtins alone.

A pin is two-sided. A commit that stamps a file in the first two counts, or
repairs a deviation, lowers that count, and `make qa` then fails on a `stale`
pin ([CI](../analysis/ci.md)). Re-pin it in the same pull request with
`elle-rig test --repin tests/ratchet/audit.lisp`, so the ledger's diff shows
how far the change moved the queue.

A rule change moves the policy date, and every file stamped before that day
joins the second count. That reading is a `regression`, which `--repin`
refuses. Edit the pin by hand in the change that moved the rule.

## A stamp is a claim the build can falsify

The stamp attests to three things a machine cannot decide. Everything else in
the policy is checkable, and [the prose checks](../../tests/prose.rs) check it.
QA runs them before anything else ([CI](../analysis/ci.md)).

Those checks run **only against files that carry a stamp**. An unstamped file
has claimed nothing and is exempt; a stamped one has claimed to meet the
policy, so a violation in it is a false claim and fails the build. Coverage
grows as the tree comes into policy, and it needs no flag day.

A deviation stamp is exempt too. It claims the content and names the issue that
tracks the rule it breaks, so a check that failed it would only repeat the
issue. The third count keeps those files from growing in number unseen.

Size is not among these checks. Every source and every document is on the size
ledger, stamped or not ([the documentation policy](../../DOCUMENTATION.md)).

This is the hedge against a stamp applied without an audit. Confirming the size
no longer discharges the claim, because the size was never the reader's job.
What is left to attest is the part that takes reading the file: whether it is
still true, whether it earns its length, and whether it holds something that
belongs elsewhere.

A stamp can still be false — an agent can read nothing and stamp a file that
happens to pass every check. The checks remove the failures that look like
diligence, so what remains is a claim somebody made about content, which a
later reader can find wrong and act on.

## Repair is the point

A stamp records that a file met the policy on a day. Applying the policy to a
file that violates it means fixing the violation, not recording the reading.

So an audit that finds a stale paragraph rewrites it, in the same change. A
fresh plain stamp over a standing violation is a false claim, and it is worse
than no stamp: it removes the file from the queue that would have brought
somebody back to it. A violation the change cannot repair gets an issue, and
the file gets the deviation stamp that names it.
