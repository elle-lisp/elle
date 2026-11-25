# The test corpus

<!-- audited: 2026-11-23 -->

What a test is to `elle test`: a form in a flat corpus of files, or an ad-hoc form a run keeps until it is promoted.

The corpus is text in git, and the store is an index derived from it
([test-store](test-store.md)). How a run compiles and runs a file is
[test-runner](test-runner.md).

## Ad-hoc tests: born at the prompt, promotable to the corpus

An agent rarely starts with a file; it probes: `elle test -e '(assert (= (foo)
42))'`. That form runs like any other and is **persisted into the index as an
ad-hoc form** — same syntax-hash identity, with `origin` set to `:adhoc`. Every
query sees it. It is *not* in git — it has no file, so no later run scans it
again.

When an ad-hoc test earns its keep, `elle test --promote <hash> <name>` renders
its syntax into `<corpus>/<name>.lisp`. Now it is durable: in git, diffable,
distributed, and re-derived as a durable form on the next scan. The motion from
throwaway probe to permanent regression test is one command, and identity is
preserved across it because both sides key on the same syntax hash.

An ad-hoc form stays in the index until `elle test --reset` removes the whole
store. Durable forms always re-derive from the corpus.

## The durable corpus is a flat set, classified by query

The design stores tests **one form per file** — file == test == syntax hash —
addressable, with their own blame and history, movable and promotable as atoms.
There is **no directory hierarchy**, and filesystem order is treated as
**semantically void**. This is the same argument made twice:

- *Order* is not a property of an isolated test. Execution order belongs to the
  runner — by hash, by failure-recency, or randomized to surface hidden
  inter-test coupling. Nothing about sequence belongs in a path.
- *Category* is not single-valued either. A form that does `(chan/send …)`
  inside `(fiber/new …)` while catching a signal is a channel test *and* a fiber
  test *and* a signal test. A directory forces one bucket and discards the rest;
  `compile/analyze` already yields the faceted truth (`touches`, `caps`, signal
  profile, [test-store](test-store.md)), so any grouping you'd draw as a tree is
  a `SELECT` over the index and a form appears in *every* view that applies.
  **Category is a query, not a directory.**

What promotion assigns is therefore a **name**, not a **place**: `touches
chan/send` + the derived label → `bounded-send-full.lisp`. A human *reads* the
failing test, and that is the one need the index can't derive away — order and
hierarchy both can. `--promote` takes the name from its caller; suggesting one
from analysis is design ([test-cli](test-cli.md)).

The split between `tests/lang/` and `tests/impl/` is not a category in this
sense: it says which suite owns a claim and which program runs it
([spec](spec.md)), which no analysis of the form can derive.

The runner compiles any file regardless of how many forms it holds (via the
multi-form compilation mode, [test-runner](test-runner.md)), so the multi-form
files in both suites run unchanged; exploding them into the
one-form-per-file shape is a mechanical codemod for when it's convenient, not a
prerequisite.
