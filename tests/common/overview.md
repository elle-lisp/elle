# Shared test helpers

<!-- audited: 2026-11-23 -->

One copy of each helper the Rust tests share, so that no test file keeps a
variant of its own.

The helpers fall into five kinds:

- an eval over a fresh `Runtime`, with or without the standard library,
  scheduled or not
- an eval over a cached `RuntimeCore`, for property tests
- the proptest configuration, whose case count `PROPTEST_CASES` overrides
- a scratch directory that is removed when it drops, and the fixture files
  written into one
- the readers of the Makefile, the two Elle suites, the suite passes a target
  runs, the CI workflow files, the documents `make doctest` runs, and an
  `elle test` session store

The test harness runs the tests, each test file defines its own cases, and
[tests/fixtures](../fixtures) holds the static files the tests read.

## Where a helper is described

Each helper carries its own docstring, and each file opens with what it holds.
[mod.rs](mod.rs) also holds the rule every one-shot eval follows: inspect a
result inside the closure the eval hands it to, while its heap is alive. It
re-exports every reader but those of [documents.rs](documents.rs).
[The tests guide](../AGENTS.md) names the helpers a new test reaches for first.
