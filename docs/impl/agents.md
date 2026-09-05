# The generated index

<!-- audited: 2026-09-05 -->

Every directory's `AGENTS.md` is built from the call-out of each document
beneath it, so the index cannot rot or be posted to.

## The problem it solves

An `AGENTS.md` here does two jobs at once. It indexes what lives in a
directory, and it holds the module's design knowledge — invariants,
constraints, the things a newcomer gets wrong. The two jobs have opposite
maintenance needs.

The index is derivable. Every fact in it already exists in the files it points
at, so a machine can rebuild it and a human never has to.

The knowledge is not derivable. Somebody learned it and wrote it down.

Because they share a file, the derivable half cannot be regenerated without
destroying the other half. So it is maintained by hand, which means it is
maintained when somebody remembers. The result is measurable: this repository
carries 48 hand-written `AGENTS.md` files totalling about 8,900 lines, and the
one under `docs/` spent months naming five documents that a single commit had
deleted.

Mixing the two also makes the file a destination. A directory's `AGENTS.md` is
the obvious place to pin a notice, and nothing costs anything to append.

## The split

**A leaf is a document.** It opens with `# Title`, then one sentence under 140
characters, then its content. It carries an audit stamp. Design knowledge that
lives in an `AGENTS.md` today becomes a leaf beside it.

**An index is `AGENTS.md`.** It is generated. It holds no prose that a person
wrote, so there is nothing in it to rot and nowhere in it to post.

An index lists, for its own directory:

- each document, as a link, with its title and call-out
- each subdirectory, as a link, with that directory's summary
- a link to the parent index

## The call-out

The call-out is the first sentence of the first paragraph under `# Title`. The
document owns it, the index quotes it, and editing the document is how the
index changes.

The budget is 140 characters. Every entry competes for the attention of a
reader who is scanning, and a repository of this size puts a hundred entries in
front of them.

A document whose call-out runs over budget, or that has none, is listed by
title alone with `(more...)`. It stays reachable, and it joins the audit queue.
Silence is the wrong failure: an index that omits a document sends the reader
to a search.

## A directory's summary

A parent index needs one line per child directory. It comes from
`<dir>/overview.md`, whose call-out serves as the directory's summary.

Where a directory has no `overview.md`, the parent lists the titles of the
documents inside it. That is worse than a summary and better than a bare name,
and it needs no judgment from the generator.

## Depth is cheap

Reading an index that points at five documents and then reading two of them
costs time. It does not cost tokens, and the tokens it saves compound over
every session that reads the two and skips the three.

So this design prefers many small documents at depth over few large ones near
the root. A document nobody navigates to costs nothing, which means a weak
document at a leaf is not a problem to solve. A weak paragraph near the root is
charged to every session, forever.

That is the reason to push a claim down rather than to delete it, and the
reason an index must be free to maintain. Hierarchy only pays when navigation
is generated.

## The index is committed

`git worktree` gives every agent a fresh checkout, and this repository keeps
several in use at once. The harness reads `AGENTS.md` before any target has
run. An ignored index is therefore absent exactly when the first session in a
new worktree needs it.

So the index is committed, and a `--check` mode fails when it is stale. The
cost is diff noise on every document that changes its first sentence. The
alternative costs every new worktree its index until somebody runs a build.

## Commands

| Command | What it does |
|---------|-------------|
| `scripts/agents [DIR]` | write the index for DIR and every directory below it |
| `scripts/agents --print DIR` | write DIR's index to stdout |
| `scripts/agents --check [DIR]` | exit non-zero when a committed index is stale |
| `scripts/agents --root DIR` | treat DIR as the repository root |

`make agents` writes every index. `make agents-check` is the CI gate.

`--root` exists so the generator can run against a fixture tree rather than
the repository that contains it. Without it the extraction rules can only be
tested by mutating the real tree, and a test that edits the tree it is checking
cannot run twice.

## Migration

The 48 hand-written indexes convert one directory at a time. Each conversion is
its own change:

1. Move the design knowledge to a leaf document, and give it a call-out.
2. Delete what the knowledge duplicated from a parent, and link instead.
3. Generate the index, and confirm it names every document in the directory.

A directory is converted when its `AGENTS.md` is generated and its knowledge
has an audit stamp. Until then the old file stands.

**The generator owns a directory whose index it wrote, and no other.** It
writes where there is no `AGENTS.md`, and where the existing one carries its
generated marker. It refuses a hand-written index and reports the path.

`--check` follows the same rule. A gate that reported every unconverted
directory as stale would be red from the day it was turned on, and it would
tell the reader to run a generator that then declines the work.

The alternative is a generator whose first run destroys every hand-written
index in the tree. Converting a directory is therefore a deliberate act:
delete the old file once its knowledge has moved to a leaf, and the generator
takes the directory from then on.
