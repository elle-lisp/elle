# Contributing to Elle

## The not rocket science rule

`origin/main` is green. Always. Every commit on main has passed every
Elle test, every Rust test, every example, and every documentation file
in CI. This is enforced by a multi-layered PR/merge-queue workflow that
runs the full suite at least three times before a commit lands.

This is the "not rocket science rule of software engineering": maintain
a repository of code that always passes all tests. It is successfully
practiced by the Rust compiler, the Linux kernel, and many other
projects. There is nothing novel about it.

### What this means for branches

No branch can be merged until it passes all tests. There are no
exceptions. If a test fails on your branch, you have two options:

1. **Fix the defect.** The test caught a bug your code introduced.
2. **Fix the test.** The test itself is wrong — its expectations are
   stale, its setup is broken, or it tests behavior that was
   intentionally changed.

There is no third option. "Skip the test" is not an option. "Mark it
known-failing" is not an option. "It's pre-existing" is not an option.

### Do not check out main to "verify" a failure

A common anti-pattern: a test fails, and the agent thinks "let me just
check if this fails on main too." This involves `git stash`, checking
out main, rebuilding, running the test, checking out the branch again,
`git stash pop`, rebuilding again. All to discover what we already
know: main passes the tests.

This is expensive, risky (stash conflicts, dirty working trees), and
pointless. Main is green. That is proven by CI on every merge. Do not
spend tokens and wall-clock time confirming a known fact. When a test
fails on your branch, your branch broke it. Fix it.

### Why "pre-existing" is not an excuse

Because main is green, every test that fails on your branch either:

- **Was introduced by your branch.** You wrote the test, or your
  changes broke an existing test. Fix it.
- **Was uncovered by your branch.** Your changes exposed a latent
  bug — perhaps a test that was order-dependent, timing-sensitive, or
  masked by a different code path. Your branch revealed it; your
  branch fixes it.

The reasoning is simple: if the defect were truly pre-existing, main
would not be green. Main is green. Therefore, the defect is not
pre-existing — it is a consequence of your branch's interaction with
the codebase.

Even in the rare case where a defect genuinely exists on main (a flaky
test that passes on CI hardware but fails on yours, a race condition
that only manifests under load), the response is the same: fix it.
If you discovered it, you are in the best position to understand and
fix it. And your branch cannot merge until you do.

### Why this matters for LLM agents

LLM agents are prone to a specific failure mode: encountering a test
failure, classifying it as "pre-existing" or "unrelated", and moving
on. This is exactly wrong. The test suite is the ground truth. When a
test fails, the test is telling you something. Listen to it.

The correct workflow:

1. Run the tests.
2. If a test fails, understand why.
3. Fix the cause — either in your code or in the test.
4. Run the tests again.
5. Repeat until green.

Do not skip tests. Do not add skip lists. Do not mark tests as
expected failures. Do not rationalize failures away. Fix them.

## Running tests

| Command | Runtime | What it does |
|---------|---------|-------------|
| `make smoke` | ~30s | Elle scripts (VM, JIT, WASM) + doctests + docgen |
| `make test` | ~3min | smoke + fmt + clippy + rustdoc + unit tests |

See [AGENTS.md](AGENTS.md) and [docs/testing.md](docs/testing.md) for
test organization, helpers, and how to add tests.

## Conventions

- Files and directories: lowercase, single-word when possible.
- Target file size: ~500 lines / 15KB.
- Prefer formal types over hashes/maps for structured data.
- Validation at boundaries, not recovery at use sites.
- Do not add backward compatibility machinery.
- Do not silently swallow errors. Propagate or log with context.
- Breaking changes are fine. Use epochs for mechanical migration.

## Making changes

1. Read the relevant AGENTS.md files for the modules you're changing.
2. Write or update tests for every behavioral change.
3. Run `make test` before committing.
4. Update AGENTS.md and docs when you change interfaces.
5. All tests pass. No exceptions.
