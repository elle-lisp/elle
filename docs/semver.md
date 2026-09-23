# elle semver

<!-- audited: 2026-09-23 -->

`elle semver` computes and verifies the version bump a library's surface change requires.

Four things share the word, and this page owns the command. `std/semver`
([lib/semver.lisp](../lib/semver.lisp)) is the version arithmetic. The
modules under lib/semver/ are the tool's own logic. A `.surface` file is
a library's released public surface. `elle semver` is the command that
reads all of them. [versioning](versioning.md) owns the declarations,
the `.surface` format, and the floor table; read it first.

A surface diff proves a lower bound — the floor — on the bump a release
must claim. Behavioral equivalence is undecidable, so the floor is one
leg of three: the diff computes the floor, the prior release's tests
arbitrate compatibility claims, and a major release ships migration
rules. This page covers all three: the dev loop, `release`, `check`, and
`migrate`.

## The dev loop

```text
elle semver [PATH]
```

`PATH` is a module file path (`lib/semver.lisp`) or an import spec
(`std/semver`, resolved as `lib/semver.lisp` under the working
directory). The command extracts the worktree surface, diffs it against
the committed `.surface` beside the module, and prints every change
under the floor it forces:

```text
std/semver  1.0.0 (.surface) -> worktree

major
  removed    satisfies?      was fn [version requirement] :signals [:error]
minor
  added      matches?        now fn [requirement version] :signals [:error]
patch
  docs       increment       "d4e5f607" -> "1a2b3c4d"

floor: major    claimed: 1.1.0    verdict: INSUFFICIENT (need >= 2.0.0)
```

The verdict compares the version the worktree module declares against
the baseline version plus the floor. An unchanged surface prints one
line:

```text
std/semver 1.0.0: surface unchanged (floor: none)
```

A module with no `.surface` beside it prints an `initial` note and
changes nothing; only `release` creates the baseline.

With no `PATH`, the command walks down from the working directory,
skipping VCS and build directories, and prints one status line per
module that declares a version. When the walk finds exactly one, it
prints that module's full diff instead.

## Cutting a release

```text
elle semver release [PATH] [--tests GLOB] [--tag]
```

`release` extracts the worktree surface and writes it as the `.surface`
baseline beside the module. Before writing, it refuses (exit 1) unless
the claim is honest:

- The claimed version must satisfy the floor over the old baseline.
- The claimed version must be greater than the baseline version. A
  re-release of an unchanged surface at the same version is idempotent
  and allowed.

A module that declares no `(elle/version ...)` is a tool error
(exit 2). There is no `--force`; fix the version.

The written file records provenance: the `HEAD` commit when the module
sits in a git repository (the form is omitted otherwise), the current
UTC date, and the test glob that pins the release — `--tests GLOB`, or
`tests/elle/<leaf>*.lisp` by default. `--tag` also creates the
lightweight git tag `<leaf>/v<version>`, which later arbitration
prefers over the recorded commit.

## Checking a claim

```text
elle semver check [PATH...] [--no-tests] [--strict] [--json]
```

`check` is the CI gate. It computes the dev-loop verdict, and then
arbitrates a compatibility claim: a patch or minor bump promises the
previous release's behavior, so the previous release's tests must pass
against the worktree code. The floor cannot see a function that keeps
its shape and changes its answers; the old tests can.

1. The baseline rev is the tag `<leaf>/v<version>` when it exists,
   otherwise the `.surface`'s recorded `(released :commit ...)`.
2. The old test set is the `.surface`'s `(tests GLOB)`, resolved at the
   baseline rev by listing that commit's tree.
3. The old files materialize in a scratch directory (removed
   afterwards) and run as `elle test` in a child whose working
   directory is the worktree, so their imports resolve through the
   ordinary search path to the new code. No import is rewritten.
4. The child failing rejects the claim: `compat claim rejected`,
   exit 1.

A major claim promises no compatibility, so arbitration is skipped.
It is gated on migration coverage instead: every major-classified
change must be named by a rule in the module's `(elle/migration N ...)`
form for the claimed major — [versioning](versioning.md) owns the rule
vocabulary. Missing coverage fails (exit 1). It prints `hint: 2 major
breaks have no migration rule; add to lib/x.lisp:` and then the skeleton
of the form to ship, which is a declaration the compiler accepts as it
stands:

```lisp
(elle/migration 2
  (rename satisfies? matches?)
  (warn parse "describe the break"))
```

A removed export whose shape an added export matches suggests a
`rename`; a removed export suggests `remove`; any other major change
suggests `warn`. A constructor change is covered by naming
`constructor`. Pre-1.0 claims are exempt — a `0.y` release promises
nothing to migrate.

When arbitration is unavailable — no repository, an unresolvable
baseline rev, no recorded tests — `check` prints a note and passes;
`--strict` turns that into exit 1. `--no-tests` skips arbitration
outright. With no `PATH`, `check` walks from the working directory
like the dashboard and checks every versioned module it finds.

## Migrating consumers

```text
elle semver migrate LIB FILE... [--check] [--dry-run] [--from N]
```

`migrate` reads `LIB`'s shipped `(elle/migration ...)` forms and
applies them to each consumer `FILE`. Rules chain ascending by major,
so a consumer two majors behind is repaired in one run; `--from N`
skips the majors at or below `N`, which the consumer already crossed.

Per file, the tool finds the top-level module bindings —
`(def m ((import "std/x") ...))`, by the path or the spec `LIB`
resolves to — and instantiates the rules per binding:

- `rename old new` rewrites the token `m:old` to `m:new`.
- `replace` rewrites calls of `m:old` at the shown arity, substituting
  `$n` with the argument source text and re-qualifying the template's
  export names with `m:`. A different arity is left alone.
- `remove` and `warn` occurrences are reported with the shipped
  message and their line; nothing is rewritten.

An import the tool cannot see through — destructured, aliased through
another binding, or dynamic — is reported for manual migration, never
guessed. `--dry-run` prints what would change and writes nothing.
`--check` writes nothing and exits 1 when a file still needs
migration or still uses a removed export.

## Exit codes

The codes separate the verdict from the tool, so CI can gate on 1
without treating a broken tree as a bad claim:

| Code | Meaning |
|---|---|
| `0` | claim sufficient, or surface unchanged, or initial |
| `1` | the verdict: claim insufficient, arbitration failed, or `release` refused |
| `2` | tool error: unreadable module, missing version form, bad `.surface` |

## JSON

`--json` prints one object on stdout instead of the report — the same
fields the report renders: `module`, `baseline`, `claimed`, `floor`,
`required`, `verdict`, and `changes`, each change carrying `export`,
`change`, `floor`, `was`, and `now`.

## The tool's modules

The logic lives in ordinary importable modules so tests and consumers
reach it without the command:

- [lib/semver/file.lisp](../lib/semver/file.lisp) — `.surface` render
  and parse, byte-deterministic.
- [lib/semver/surface.lisp](../lib/semver/surface.lisp) — hybrid
  extraction: runtime reflection first, static analysis for names and
  as the `:static` fallback.
- [lib/semver/diff.lisp](../lib/semver/diff.lisp) — the floor table as
  code, plus the claim verdict.

Only the thin driver in src/semver is embedded in the binary, the same
way `elle test` embeds its runner.

The pieces compose without the command. The diff of two surfaces names
each change and its floor:

```lisp
(def sdiff ((import "std/semver/diff")))
(def old {:format 1 :module "m" :version "1.0.0" :mode :hybrid
          :exports {:f {:kind :fn :required 1 :optional 0 :rest :none
                        :named-keys [] :params ["a"]
                        :signals {:bits [] :propagates []}}}})
(def new (put old :exports {}))
(def d (sdiff:diff old new))
(assert (= (d :floor) :major) "removing the only export forces a major")
(assert (= ((first (->list (d :changes))) :export) :f)
        "the change names its export")
```

The floor plus a baseline gives the lowest sufficient claim, with
cargo's pre-1.0 mapping:

```lisp
(assert (= (sdiff:required "1.2.3" :major) "2.0.0") "post-1.0 major")
(assert (= (sdiff:required "0.3.1" :major) "0.4.0") "pre-1.0 major bumps y")
(assert (= (sdiff:required "0.3.1" :patch) "0.3.2") "pre-1.0 patch bumps z")
```

And the verdict is one comparison, computed with `std/semver`:

```lisp
(def v (sdiff:verdict {:baseline "1.2.3" :claimed "1.3.0" :floor :major}))
(assert (= (v :verdict) :insufficient) "1.3.0 does not cover a major break")
(assert (= (v :required) "2.0.0") "the verdict names the requirement")
```

## Checking every library

```text
make semver-check
```

The target runs the built binary over every module in the tree that
declares a version and fails on any insufficient claim — the same walk
as the zero-argument command, gated.

---

## See also

- [versioning](versioning.md) — declarations, the `.surface` format,
  and the floor table
- [modules](modules.md) — the closure-as-module pattern the extractor
  instantiates
