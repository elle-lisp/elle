# Value lifetime, constants, and teardown — the guarantees

This is the consumer's view of how long things live: what persists for the whole
process, why your "constants" are not eternal, and what is true after your
program ends. The machinery behind these guarantees — how literals lower as
ordinary allocations, the teardown sweep — is in
[docs/impl/region/model.md](../impl/region/model.md) and
[docs/impl/region/rules.md](../impl/region/rules.md).

## The naive user model

Running a file behaves as if it were one evaluation:

```
elle foo.lisp   ≡   (eval (wrap-in-letrec (read-all (slurp "foo.lisp"))))
```

After that `eval` completes and its result is dropped, the world is back to where
it started: every region the run created is freed, and the only thing that
persists is what was there *before* `main` and will be there *after* — the
**native-function primitives**. Nothing your program allocates is eternal.

## Your "constants" are not eternal — compile-time means immutable, not forever

A string literal, an array literal, a quoted form, a closure template is
**compile-time-constant** in the sense that it is *immutable* — never in the
sense that it lives forever. Runtime `(eval …)` and module load re-run the whole
compiler, so "compile time" is not a moment that happens once; it recurs. Each
literal is therefore an **ordinary allocation**: it is born in its own region,
freed at its last use, and kept alive by reference counting only if it escapes
(into a container, a closure, a returned value). A literal that escapes its scope
or enters a mutable container is kept alive; one that does not is freed at its
last use — exactly like any other value.

The practical takeaway: do **not** assume a literal is interned or shared
process-wide. If you evaluate the same source repeatedly, each evaluation
materializes its own copies, and each copy is reclaimed when it falls out of use.
There is no constant pool that grows without bound (and if you ever observe one
that does, that is a leak to report, not a feature).

## After your program ends, everything frees

When your program — or any single `(eval …)` — completes and its result is
dropped, every region is freed; the live region count returns to its pre-run
baseline. Even the **stdlib, prelude, core
environment, and trait tables** are torn down before the process exits; they are
resident *roots* held for the process's life, not eternal values.

This same guarantee holds identically on every way of running Elle: a file
(`elle foo.lisp`), a REPL session you exit cleanly, the embedding API, and the
lint path. (The one deliberate exception is the long-lived LSP server, which
keeps a single runtime alive for the editor session.) If a region is still live
after a run finishes, that is a leak.
