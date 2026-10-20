# Documentation policy

<!-- audited: 2026-10-19 -->

Every document and every source file says what is true now, names the document
that governs it, and carries the day it last met these rules.

## Why this pays

A pull request is a session, and every session starts cold. A paragraph you
correct today is one the next several hundred sessions never work around. A
stale sentence is a tax each of them pays, and the ones that believe it write
code somebody reverts.

Delivery decides most of the cost. The same fact costs a session ten tokens or
four hundred, depending on where it sits and whether the reader reaches it in
one step.

## Repair on discovery

**A document you find wrong is yours.** Fix it inside the change you are
already making.

- Correct the sentence in place. A correction appended beside it leaves the next
  reader holding two claims and no way to choose.
- Delete what no longer describes the code.
- When the repair is larger than your change, file an issue and cite it where
  the claim sits.
- Audit against this file as it stands today.

## Auditing

**Touch a file, audit it.** Read it whole, apply these rules, then stamp
today's date.

A machine can check file length, broken links, bare filenames, missing
call-outs, banned wording, over-long sentences, and a sentence repeated in two
files. A repository's own section says which of these its build checks, and the
stamp claims the rest by hand. The stamp always claims the three a machine
cannot decide:

1. Every claim in this file is still true of the code.
2. The content earns the time it takes to read.
3. Nothing here belongs in another file.

Confirming the line count and stamping is the failure this list exists to
prevent. A file can sit inside every limit and still describe an interface that
changed a year ago.

**A false stamp is worse than none.** An unstamped file waits in the queue for
somebody to reach it. A stamped one has left the queue, so a wrong claim stops
anybody being sent back.

A source file carries the stamp in its header block. A document carries it in an
HTML comment under the title.

```
// audited: 2026-09-04
// Renders the debug pane, and gates it out of a release build.
// design/ui/debug.md
```

The date is ISO 8601, and a file with no stamp sorts to the front of the queue.
Break a rule with no issue behind it, and remove the stamp. A file that cannot
meet a rule yet stamps `audited: <date> (#N)`, which reads as audited with a
known deviation, tracked where the work is scheduled.

A file comes into policy because somebody opened it for another reason and held
it to the rules on the way past. The queue proposes an order; what you touch
decides the real one. A file stamped before a rule was written has never met
that rule, so the stamps measure how far a change here has spread.

## Links

**Write every reference to a file as a markdown link.**

```
yes:  [the debug pane](design/ui/debug.md)
no:   `design/ui/debug.md`
```

A link is a pointer the reader follows in one step, and a tool can ask whether
it still resolves. A filename in backticks costs the reader a search, no tool
can see it, and it goes wrong silently.

Link the document, never a section or a line inside it. Documents are read
whole, so a finer pointer goes stale the next time that document is edited.

**A citation in source names a file and nothing inside it.** Every file stays
under the cap so that a reader reads it whole. A quoted section invites a read
of that section alone, and it breaks the day somebody renames the heading.

## Moving a file

**When you move, rename or delete a file, sweep every reference to its old path
in the same change.** One commit that reorganizes a directory orphans every
index entry and cross-reference at once, and it is titled for the feature it
ships, so nobody thinks to open the index.

## Reading

- Find the document that owns a subject in the index, then read it whole. The
  index is `AGENTS.md` — the nearest one, where a repository keeps several.
- Read a file end to end before you edit it. The size cap makes that
  affordable.
- Scope a search to a directory or a glob.

## Placement

- Keep a document beside the code it describes.
- Open a document with `# Title`, then one sentence under 140 characters. That
  sentence is the call-out, and it is how an index describes the document. Write
  it in the plainest words the subject allows.
- Put a claim at the deepest place that owns it. A fact about one function
  belongs in that function's docstring, where the reader who calls it looks.
- Promote a claim upward only when a reader who is not working on that subject
  still needs it. Promotion charges every session, so state the reason.

## One argument, one home

**Every behavior has one document that states it.** A second document that
needs the fact links to the first and stops. It restates nothing, not in
summary and not as a row of its own table. A correct copy is still a copy: the
day the behavior changes, the copy nobody edited becomes the lie, and a reader
cannot tell which of the two is true.

Write a design decision down once. The design document owns the argument and the
alternatives it beat. A code comment says only what a reader of that file would
otherwise get wrong, and cites the document instead of restating it. A test
states its claim in its name and proves it in its body.

No sentence appears in two artifacts. The bill arrives on the day the decision
reverses: every copy has to be rewritten, and the copies you miss become lies
that nothing fails on.

**A document about a thing says what the thing is.** Where that thing is used
belongs to the surface that uses it. A document carrying its own call sites goes
stale every time one moves.

**Name the other document and stop.** One document never speaks for another,
even to agree with it.

**A design document states no provenance.** It says what the product does and
why, and the why is a clause. The measurement that settled a decision lives in
its report, and the report names the design it informed. A design document that
cites a report copies a dated number into a file about now, and git already
holds when and why the design moved.

## How a document reads

- **State what is.** An alternative that lost earns a clause, never a section.
- **A reason is a clause.** "because nobody has planned it" carries what a
  paragraph of argument would have.
- **Facts go in a table.** Prose carries what a table cannot hold.
- **A paragraph does not announce itself.** A bold thesis in front of every
  paragraph is a table of contents the reader did not ask for.
- **Cut words until the meaning is at risk**, then stop.

A document is as long as its content, and the cap below is a ceiling rather than
a target.

## Documents

- Name files and directories in lowercase, one word where the word exists.
- Keep a source file under the token cap the repository's own section states,
  and a document under 500 lines. Where a repository allows an exception, its
  own section names the ledger that records it.
- A size ledger only goes down. Never thin the comments to fit.

The cap is a reading budget. It exists so that reading a file end to end stays
affordable, because reading it end to end is what produces an understanding the
file can then be edited from. So a long file is a question rather than a
verdict: does this hold more than one subject? Usually it does, and splitting it
is the answer. Sometimes the number is wrong for that file.

A source's cap is in tokens, because a token is what a reader pays, and what a
line costs in tokens varies twofold by kind. The cap sits where sources
start to name three governing documents more often: past it, more than one
source in twenty does. A document's cap stays in lines until a document has a
one-subject proxy of its own.

### The remedies, in order

1. Delete what the file no longer needs. This is the only remedy that leaves the
   repository smaller.
2. Move a subject out, with the tests that cover it. The lines land where a
   reader looking for that subject already goes.
3. Split the file where it has no seam. Take this one last.

A split writes a header, an import block and a `mod` line, so the directory
around it grows. Where a repository budgets a directory as well as a file,
charge that seam to the directory. Raise a directory budget in the change that
grew it, and name the subject that arrived.

### A table grows by rows

A file can sit over the cap because it holds one row per fact from outside the
repository: a dispatch over wire bytes, a specification table. None of the three
remedies reaches it, because deleting a row deletes a fact.

Three things admit such a file, and all three have to hold. The file is a
dispatch or a constant table over facts the repository does not own. The code
behind one row is bounded. A split would separate the rows from their reader. A
file that dispatches and also works inside each arm splits instead.

An admitted file says so in the size ledger, in a list of its own, and the entry
names the rows. That entry may go up, in a change that adds rows and says which.
It leaves the total the rest of the tree owes, because that total is what has to
fall. Such a file keeps its plain stamp.

## Source comments

The next reader arrives cold, with no memory of this session. The code shows the
what; a comment carries the why.

- **Open a source file with its call-out**, in the header block: one sentence
  under 140 characters saying what the file is for. A reader arrives at it from
  a stack trace, a grep, or a call site, with no index to have prepared them.
- Name the governing documents in the same header, once, and at most three. A
  file that needs more does more than one job.
- Write the reference as a plain path. No comment syntax carries a link, so
  these are the one place a bare filename is correct — and the one place a build
  has to check the path.
- Write inline comments about the implementation alone.
- Never cite a defect, a leak, or a fix. **A test is the reference for a
  defect.** It demonstrates the problem and goes green when the problem is gone;
  a comment starts lying that day instead.
- Never cite a working plan, a hand-off note, or a numbered stage. The cold
  reader cannot resolve any of them.
- Never narrate a change. Git holds what the tree used to be.

A test comment owns two things, because nothing else can hold them. **The
trap**: a platform behavior you discovered painfully, recorded beside the
assertion that guards it. **The counter-factual**: the wrong answer that looked
right, and what the test would have missed. It does not own the argument.

## Claims that can run

Where the build executes documentation, a claim inside a code fence cannot go
stale without failing, and the repository's own section names the target that
runs it. Nothing checks prose, so put a claim inside a fence whenever it fits one
— an arity, a return value, an error message. A worked example that runs is worth
three sentences that assert.

## Writing rules (ASD-STE100, adapted)

Mandatory for all prose written here: documents, commit messages, comments,
reports, error messages, and what you say to the user. Never applies to code,
identifiers, or quoted text.

### Hard rules

1. Maximum 30 words per sentence in instructions, 35 in descriptions.
2. Active voice. Passive only when the agent of the action is unknown.
3. Instructions use the imperative: "Run the tests."
4. One instruction per sentence. Condition first, comma, then command.
5. One name per concept, used consistently through the whole text. Never invent
   synonyms.
6. Use the project's established terms. New names: three words maximum.
7. No noun stacks over three words. Break them or hyphenate the unit.
8. Simple tenses only. Use a verb for an action, not a noun: "compress the
   file", not "perform compression".
9. Use articles ("the buffer"), not telegram style ("acquire lock").
10. One topic per paragraph, six sentences maximum. Lead with what the reader
    needs first.

### Word choice — say this, not that

- use, not utilize or leverage
- start, not initiate or kick off
- show, not surface or expose (unless technical)
- do, not perform or action
- remove, not take out; continue, not carry on (no phrasal verbs)
- for example, not e.g.; that is, not i.e.
- make sure that X, not ensure X (keep "that" after make sure / show /
  recommend)
- repeat the noun when "it" or "this" could point at two things
- American English spelling

Warnings state the command or condition first, then the risk: "Do not run this
on production. It deletes the table." Notes give information only, never
instructions.

### Banned register

Inflated assistant-speak is banned. When you catch yourself writing one of
these, state the plain fact instead:

- "load-bearing", "smoking gun", "delve", "nuanced", "crucially",
  "fundamentally", "parsimonious", "legible" (for non-text),
  "directionally *", "first-order/second-order" (outside math),
  "productive tension", "recalibrate", "reframe"
- Throat-clearing: "put differently", "to be candid", "stepping back",
  "zooming out", "the key distinction is" → delete; say the thing once.
- Apology ceremony: "That's on me", "You're right to push back", "I should have
  caught that" → give the correction, skip the ceremony.
- "It is not X so much as Y" / "This is not X. It is Y." → say what it is.
- Confidence theater: "I am not yet convinced", "the most responsible position
  is probably" → say "I do not know" or state what you know.
- One-sentence paragraphs for drama; chiasmus endings ("That is not a failure
  of the analysis. It is the analysis.") → if the text needs a flourish to
  land, the content is missing.

Two more that this repository adds:

- Write the process the project follows. A named shortcut reads as an option,
  so leave it out.
- Write standing process here. A tool reports today's state.

The test: delete a sentence. If the meaning does not change, it was
decoration — keep it deleted.

### Example

Bad:
> You're right to push back here. Stepping back, the load-bearing issue is not
> the timeout itself so much as what it reveals about the retry logic.
> Crucially, the fix needs to operate at the right level of abstraction.

Good:
> The timeout is a symptom. The retry loop never resets its backoff counter, so
> the third attempt always exceeds the deadline. Fix the reset in
> `retry.c:142`.

## Commit messages

A commit message is a subject line, a blank line, then a body.
`git log --oneline`, a rebase list and a pull request's commit list show the
subject alone, so the subject is the line every reader scans.

- Keep the subject to 55 characters or fewer, prefix included. Count it before
  you commit.
- Start the subject with the prefix the log already uses for that area.
- Name the change in the subject. Put the reasons, the parts and the numbers in
  the body.
- When the change does not fit in 55 characters, split the commit or move the
  clauses into the body. Never let the subject run long.
- Wrap the body at 72 columns. It says why, and what the diff cannot show.
- Do not list the diff in the body. Git already holds every line it changed.
- When the subject says everything, leave the body out.

```
yes:  weather: the page states the sky at the last fix

      The page sits between tours and world, and reads the wind in
      the rider's unit under the core's arrow. Each class word says
      whether its reading is fresh, stale or absent.

no:   weather: the page sits between tours and world and states the sky …
```

## Actions

- Add every action to the `Makefile`, with a `##` help line.
- Run `make help` to find an action.
- Call the target. Composing the command from the `Makefile` source skips what
  the target does.


## This repository

Everything above is the portable policy, and is identical in every repository
that carries this file. These rules belong to this one.

- A source's cap here is 2,500 tokens. A source is a file the audit queue walks
  that is not a document: a `.rs` or `.lisp` file, or a script under `scripts/`.
  [The cap report](docs/analysis/budget.md) reads the figure off this tree, and
  `make budget` prints the curve it reads.
- [The size producer](tests/ratchet/size.lisp) reads every source over the cap
  at its 500-token bucket, rounded up. The rows of `tests/ledger/size-*.lisp`
  are the size ledger, and [the ratchet](docs/ratchet.md) judges each reading
  against its row in both directions.
- A source past its row is a `regression`, which the remedies above answer, and
  a source a whole bucket under its row is `stale`. A source back under the cap
  leaves its row `missing`, so delete the row.
- One more row pins the excess over the cap: each bucket less the cap, summed.
  `elle-rig test --repin` adopts the row of a source new to the ledger, and the
  excess then reads as a `regression`, which it refuses.
- The size producer holds the list a table is admitted to, because the runner
  alone reads the ledger. A table's excess is a reading of its own, apart from
  the excess that has to fall.
- [The prose checks](tests/prose.rs) hold every stamped file to the rules a
  machine decides here. Every link resolves, no document is named in a bare
  code span, and every call-out is present and under 140 characters.
- The same checks find no banned register, every document a source header
  names, and every document under 500 lines. A file stamped with a deviation is
  exempt from all of them. The stamp claims sentence length and repeated
  sentences by hand.
- [The audit queue](docs/impl/audit.md) owns `scripts/audit`: the queue, the
  commit gate, and the three files exempt by name. `scripts/audit --counts`
  prints the files with no stamp, the files stamped before this policy, and the
  files stamped with a deviation. [The audit producer](tests/ratchet/audit.lisp)
  pins each count.
- Every `AGENTS.md` but the root one is generated by `scripts/agents` and
  committed. Edit the document, never the index, and run `make agents` to
  rebuild it. `make qa` fails on an index that is behind the tree.
- `make doctest` runs every `lisp` fence in the documents
  [the root index](AGENTS.md) names, so a claim in one fails the build when it
  goes stale.
- A pull request lands as one squashed commit. Its title is the subject, and
  GitHub appends ` (#N)`, so keep a title to 47 characters or fewer. Its body is
  the commit's body.
