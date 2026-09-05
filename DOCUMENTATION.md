# Documentation policy

<!-- audited: 2026-09-05 -->

Every document and every source file says what is true now, names the document
that governs it, and carries the day it last met these rules.

## Why this pays

A pull request is a session, and a repository of this age has closed hundreds
of them. Every session starts cold, spends its first tokens working out where
it is, and pays again for every sentence that sends it the wrong way.

That cost multiplies by the number of sessions still to come. A paragraph you
correct today is a paragraph the next several hundred sessions never work
around. A stale sentence you leave standing is a tax each of them pays, and the
ones that believe it write code somebody reverts.

Delivery decides most of the cost. The same fact costs a session ten tokens or
four hundred, depending on where it sits, how it opens, and whether the reader
reaches it in one step.

## Repair on discovery

**A document you find wrong is yours.** Fix it inside the change you are
already making.

- Correct the sentence in place. Appending a correction beside it leaves the
  next reader holding two claims and no way to choose.
- Delete what no longer describes the code. A deleted paragraph costs nothing
  to read.
- When the repair is larger than your change, file an issue and cite it where
  the claim sits.
- Audit against this file as it stands today, never against the version you
  remember.

An agent that read a document, saw it was stale, and moved on paid the tokens
and threw away the finding.

## Auditing

**Touch a file, audit it.** Read it whole, apply these rules, then stamp
today's date.

### What the stamp claims

A build checks what a build can: file length, broken links, bare filenames,
missing call-outs, banned wording, over-long sentences, a sentence repeated in
two files. None of that is yours to attest, and none of it is what the stamp
is for.

The stamp claims the three a machine cannot decide:

1. Every claim in this file is still true of the code.
2. The content earns the time it takes to read.
3. Nothing here belongs in another file.

Confirming the line count and stamping is the failure this list exists to
prevent. A file can sit inside every limit and still describe an interface
that changed a year ago.

**A false stamp is worse than none.** An unstamped file waits in the queue for
somebody to reach it. A stamped one has left the queue, so a wrong claim does
not wait — it stops anybody being sent back.

A source file carries the stamp in its header block. A document carries it in
an HTML comment under the title.

```
// audited: 2026-09-04
// Renders the debug pane, and gates it out of a release build.
// design/ui/debug.md
```

The date is ISO 8601. A file with no stamp is not audited yet, and it sorts to
the front of the queue.

**A file that breaks a rule carries no stamp.** There is no form that records
the break and keeps the stamp, and no way to hand the repair to somebody else
and stamp the file anyway.

The stamp is what takes a file out of the queue. So a stamp over a standing
violation is exactly how that violation stops being anybody's work, and the
file that most needs a reader is the one nothing will send a reader back to.
Repair the file, or leave it unstamped where the queue can still reach it.

### The tree comes into policy one file at a time

Nobody audits this repository in a sitting. A file comes into policy because
somebody opened it for another reason and held it to the rules on the way past.
The stamp therefore rides on the commit rather than on a schedule.

The queue proposes an order; what you touch decides the real one.

### The date says which policy the file met

These rules change. A file stamped before a rule was written has never met that
rule, however recently somebody read it. The stamps therefore measure how far a
change to this document has spread into code that already exists.

## Links

**Write every reference to a file as a markdown link.**

```
yes:  [the debug pane](design/ui/debug.md)
no:   `design/ui/debug.md`
```

A link is a pointer the reader follows in one step, and a tool can ask whether
it still resolves. A filename in backticks costs the reader a search, and no
tool can see it at all. Bare names go wrong silently and stay wrong.

Link the document, never a section or a line inside it. Documents are read
whole, so a finer pointer buys the reader nothing and goes stale the next time
that document is edited.

## Moving a file

**When you move, rename or delete a file, sweep every reference to its old path
in the same change.**

Documentation rots in bursts. A single commit that reorganizes a directory
orphans every index entry and cross-reference that named the old paths, all at
once. Such a commit is usually titled for the feature it ships, so nobody
thinks to open the index, and the damage sits there for months.

The sweep is mechanical, and it belongs to the change that caused it.

## Reading

- Find the document that owns a subject in the index, then read it whole. The
  index is `AGENTS.md` — the nearest one, where a repository keeps several.
- Read a file end to end before you edit it. The 500-line cap makes that
  affordable.
- Scope a search to a directory or a glob.

## Placement

- Keep a document beside the code it describes.
- Open a document with `# Title`, then one sentence under 140 characters. That
  sentence is the call-out, and it is how an index describes the document.
- Put a claim at the deepest place that owns it. A fact about one function
  belongs in that function's docstring, where the reader who calls it looks.
- Promote a claim upward only when a reader who is not working on that subject
  still needs it. Promotion charges every session, so state the reason.

## One argument, one home

Write a design decision down once. The design document owns the argument and
the alternatives it beat. A code comment says only what a reader of that file
would otherwise get wrong, and cites the document instead of restating it. A
test states its claim in its name and proves it in its body.

No sentence appears in two artifacts. The bill arrives on the day the decision
reverses: every copy has to be rewritten, and the copies you miss become lies
that nothing fails on.

## Documents

- Name files and directories in lowercase, one word where the word exists.
- Keep a source file or a document under 500 lines. Where a repository allows
  an exception, its `AGENTS.md` names it.
- A size ledger only goes down. Split the file rather than raise its
  allowance, and never thin the comments to fit.

The cap is a reading budget, not a target. It exists so that reading a file end
to end stays affordable, because reading it end to end is what produces an
understanding the file can then be edited from. A file that grows to 499 lines
and parks there has met the number and lost the point.

So a long file is a question, not a verdict: does this hold more than one
subject? Usually it does, and splitting it is the answer. Sometimes it does
not, and the number is wrong for that file.

## Source comments

The next reader arrives cold, with no memory of this session or the plan that
drove it. The code shows the what. A comment carries the why.

- **Open a source file with its call-out**, in the header block: one sentence
  under 140 characters saying what the file is for. A document earns one
  because a reader chooses it from an index; a source file earns one because a
  reader arrives at it from a stack trace, a grep, or a call site, with less
  context and no index to have prepared them.
- Name the governing documents in the same header, once, and at most three. A
  file that needs more does more than one job.
- Write the reference as a plain path. No comment syntax carries a link, so
  these are the one place a bare filename is correct — and the one place a
  build has to check the path, because nothing else can.
- Write inline comments about the implementation alone.
- Never cite a defect, a leak, or a fix. **A test is the reference for a
  defect.** It demonstrates the problem, proves whether the problem is present,
  and goes green when the problem is gone. A comment does none of that, and it
  starts lying the day the defect is fixed.
- Never cite a working plan, a hand-off note, or a numbered stage. The cold
  reader cannot resolve any of them.
- Never narrate a change. Git holds what the tree used to be.

A test comment owns two things, because nothing else can hold them. **The
trap**: a platform behavior you discovered painfully, recorded beside the
assertion that guards it. **The counter-factual**: the wrong answer that looked
right, and what the test would have missed.

A test comment does not own the argument. One that re-derives why the design is
right is a second copy of the document, it is not executable, and nothing fails
when it rots.

## Claims that can run

Where the build executes documentation, a claim inside a code fence cannot go
stale without failing. `AGENTS.md` names the target that runs it.

Prose is most of any document, and nothing checks prose. So put a claim inside
a fence whenever it fits one — an arity, a return value, an error message. A
worked example that runs is worth three sentences that assert.

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

## Actions

- Add every action to the `Makefile`, with a `##` help line.
- Run `make help` to find an action.
- Call the target. Composing the command from the `Makefile` source skips what
  the target does.
