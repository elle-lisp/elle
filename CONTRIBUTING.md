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

If the defect were truly pre-existing, main would not be green. Main
is green. Therefore, the defect is a consequence of your branch's
interaction with the codebase — either you broke a test, or you
exposed a latent bug. Either way, fix it before merging.

The correct workflow:

1. Run the tests.
2. If a test fails, understand why.
3. Fix the cause — either in your code or in the test.
4. Run the tests again.
5. Repeat until green.

Do not skip tests. Do not add skip lists. Do not mark tests as
expected failures. Do not rationalize failures away. Fix them.

## Debugging with tests and assertions

Some bugs — especially UAF, race conditions, and timing-dependent crashes
— require enormous context to fully understand. A single debugging session
is unlikely to solve them. The correct strategy is **progressive
constraint**:

1. **Add tests to the test suite.** Every partial reproduction, every
   minimized case, every boundary condition you discover becomes a
   permanent test. Even if the bug isn't fixed this session, the test
   stays. Future sessions inherit a smaller search space.

2. **Add assertions to the code.** Rust `debug_assert!`, runtime checks,
   invariant guards — anything that turns a silent corruption into a loud
   panic closer to the site of the bug. Assertions are disposable
   scaffolding; keep the ones that catch real problems, remove the rest
   after the fix lands.

3. **Run the tests after every change.** Not at the end. Not after
   "one more thing." After every change. The tests are the compass.

The goal is not to solve the bug in one pass. The goal is that every
session leaves the codebase better defended than it was before. Bugs
have fewer places to live. Incorrect assumptions become asserts. The
search space shrinks. Eventually the bug has nowhere left to hide.

This is not optional. If you spent a session debugging and added zero
tests and zero assertions, the session was wasted.

## Running tests

| Command | Runtime | What it does |
|---------|---------|-------------|
| `cargo test -p elle --lib` | ~1.5min | Rust unit tests — the fast inner loop |
| `make smoke` | ~30min release | Elle corpus (VM, JIT) + doctests + embedding |
| `make test` | smoke + ~5min | smoke + fmt + clippy + macOS cross-check + rustdoc + unit + integration tests |
| `make crosscheck` | ~1min | Clippy over the macOS `cfg(target_os)` arms a Linux build never compiles |

Pass the release binary to anything that runs the corpus — the debug default
takes hours rather than ~30 minutes:

```sh
make smoke-elle ELLE=./target/release/elle CARGO_PROFILE=--release
```

Never read a batched suite's exit status through a pipe: `make smoke-elle |
tail` reports `tail`'s exit, not the suite's.

See [AGENTS.md](AGENTS.md) and [docs/testing.md](docs/testing.md) for
test organization, helpers, and how to add tests.

## Plugins and the stable ABI

Plugins depend on the `elle-plugin` crate — **not** on `elle`. This
provides a stable ABI: plugins can be compiled separately from elle
and loaded at runtime.

The ABI uses a named function lookup pattern (like `vkGetInstanceProcAddr`).
Plugins resolve API functions by name at init time. Adding functions to
elle never breaks existing plugins.

```toml
# Plugin Cargo.toml
[dependencies]
elle-plugin = { path = "../../elle-plugin" }  # NOT elle
```

See [`docs/plugins.md`](docs/plugins.md) for the full list and
[`docs/cookbook/plugins.md`](docs/cookbook/plugins.md) for the recipe.

## Formatting

All `.lisp` files are formatted with `elle fmt`. This is enforced by CI
and by a pre-commit hook.

| Command | What it does |
|---------|-------------|
| `make fmt` | Format all Elle source in-place |
| `make fmt-check` | Verify formatting (used in CI, exits 1 on diff) |

A pre-commit hook in `.githooks/` auto-formats staged `.lisp` files on
commit. After cloning, enable it with:

```sh
git config core.hooksPath .githooks
```

## Conventions

- Files and directories: lowercase, single-word when possible.
- Target file size: ~500 lines / 15KB.
- Prefer formal types over hashes/maps for structured data.
- Validation at boundaries, not recovery at use sites.
- Do not add backward compatibility machinery.
- Do not silently swallow errors. Propagate or log with context.
- Breaking changes are fine. Use epochs for mechanical migration.

## Comments: write for the cold reader

Every line of code and text here is written by an agent, and read by another
agent that arrives **cold** — no memory of this session, this mission, the plan
that drove it, or who wrote what. That single fact decides what a comment may say.
The reader can see the *what*; a comment exists to explain the *why*. It can
resolve a reference to code, a spec, or a test — those are in front of it. It
**cannot** resolve a reference to anything that lived only in the writing
session's head. So a comment must be self-contained and timeless: describe the
code as it is now, never the journey that produced it (git holds history).

Three references in particular are unresolvable to the cold reader and so are
forbidden — in comments and in shipped docs alike:

- **Defects and leaks.** Never write "this avoids the UAF in `foo.lisp`", "the
  dominant leak is X", or "RED until Y is fixed". **The canonical reference for a
  defect or leak is a test.** A test explains the source and state of the code
  extensively, demonstrates the problem, and proves whether it is present — and,
  because it goes green when the defect is fixed, it cannot lie. A comment can:
  the moment the defect is fixed (or never existed), a defect-narrating comment
  misleads the cold reader into distrusting correct code, and demands a follow-up
  edit to remove. State the invariant the code upholds *positively*; if a behavior
  guards against a hazard, let the pinning test carry the detail.

- **Dev-scratch files.** Never cite a working roadmap, hand-off note, or mission
  plan — the ephemeral documents that drive a session and get deleted when their
  work lands. The reference dangles the instant the file is gone, and until then
  it imports a transient framing the reader has no way to evaluate. Cite the
  permanent spec (`docs/impl/*.md`) or the test instead. When a working plan
  graduates into a spec, its citations graduate with it.

- **Session and mission scaffolding.** Never reference the numbered steps of the
  effort that produced the code — "Stage 3", "A3.2", "addendum 2", "the commit-4
  variant" — nor which session or attempt did something. The reader has no access
  to that sequence and cannot reconstruct it; it is pure noise that implies an
  ordering no longer present in the code. Provenance is equally irrelevant: the
  codebase is the artifact, not the history of who touched which line when.
  Describe the *mechanism*, not the step or session that produced it.

Graduating a working plan into a permanent spec means stripping all three: the
defect ledger, the scratch-file citations, and the stage numbers. What remains is
the mechanism as it stands, with tests as the reference for every claim about
correctness.

## Making changes

1. Read the relevant AGENTS.md files for the modules you're changing.
2. Write or update tests for every behavioral change.
3. Run `make test` before committing.
4. Update AGENTS.md and docs when you change interfaces.
5. All tests pass. No exceptions.
