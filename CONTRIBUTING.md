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
| `make smoke` | ~30s | Elle scripts (VM, JIT, WASM) + doctests + docgen |
| `make test` | ~4min | smoke + fmt + clippy + rustdoc + unit + integration tests |

See [AGENTS.md](AGENTS.md) and [docs/testing.md](docs/testing.md) for
test organization, helpers, and how to add tests.

## Plugins and the stable ABI

Plugins depend on the `elle-plugin` crate — **not** on `elle`. This
provides a stable ABI: plugins can be compiled separately from elle
and loaded at runtime. The old single-cargo-invocation rule no longer
applies.

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

## Making changes

1. Read the relevant AGENTS.md files for the modules you're changing.
2. Write or update tests for every behavioral change.
3. Run `make test` before committing.
4. Update AGENTS.md and docs when you change interfaces.
5. All tests pass. No exceptions.
