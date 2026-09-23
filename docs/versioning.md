# Versioning

<!-- audited: 2026-09-23 -->

How an Elle library declares its version and ships migration rules.

Elle files are self-describing: `(elle/epoch N)` rides inside the file
because no manifest owns Elle builds ([lexicon](impl/lexicon.md)). A
library's version follows the same rule. The version is a form in the
module file, the released surface is a committed `.surface` file beside
it, and a major release carries its own migration rules. The
`elle semver` tool reads all three.

## Declaring a version

Put `(elle/version "X.Y.Z")` at the top level of the module file, by
convention right after the epoch declaration. A module at 2.1.0 therefore
opens with `(elle/epoch 12)`, then `(elle/version "2.1.0")`, then the module
closure that returns its export struct.

The compiler consumes the declaration during compilation, exactly as it
consumes `(elle/epoch N)`. The running program never sees it. The
version string must be a literal; a file declares at most one version.

```lisp
(elle/version "1.0.0")
(assert (= (+ 1 2) 3) "the declaration is consumed and the file runs")
```

The zero-argument `(elle/version)` keeps its existing meaning: it
returns the interpreter's own version. Only the one-argument literal
form declares.

```lisp
(assert (string? (elle/version)) "the query form still answers")
```

## Shipping migrations

A major release breaks consumers, so it ships the mechanical repair.
One `(elle/migration N ...)` form per major, written when major `N`
ships and never edited after. Rules name export keys as bare symbols:

```lisp
(elle/migration 2
  "satisfies? became matches? with swapped arguments"
  (rename satisfies? matches?)
  (replace (bump $1 $2) (increment $2 $1))
  (remove legacy-parse "use parse; legacy-parse dropped :pre handling")
  (warn parse "parse now signals :parse-error instead of :semver-error"))
(assert true "migration declarations are consumed too")
```

| Rule | Meaning |
|---|---|
| `(rename old new)` | The export key `old` became `new`. A consumer's `m:old` becomes `m:new`, and destructure keys follow. |
| `(replace (old $1 ...) template)` | Calls of `old` at the shown arity rewrite into the template. `$n` names the nth argument. A different arity is left alone. |
| `(remove name "msg")` | The export is gone with no mechanical repair. Each use is reported with the message. |
| `(warn name "msg")` | The export stayed but behaves differently — a break no rewrite can express. Reported once per consumer file. |

`warn` exists so that migration coverage can be complete without
pretending every break is rewritable. `elle semver check` requires every
major-classified change to be named by some rule.

Rules chain: a consumer two majors behind applies the older form's rules,
then the newer form's, exactly as epoch migrations chain
([epochs](epochs.md)). The optional leading string is the summary a
report prints.

## The .surface file

`elle semver release` records the released public surface as a
`.surface` file committed beside the module — `lib/semver.surface` beside
`lib/semver.lisp`. Reviewers see surface changes as ordinary diff lines,
and the working tree diffs against it without any git archaeology.

The file is a sequence of Elle forms, one per line, fully sorted: header
forms in fixed order, then one `export` form per export, sorted by name.
`std/semver/file` reads and writes it, and a file it reads renders back to
the same bytes:

```lisp
(def sfile ((import "std/semver/file")))
(def surface-text
  (string (string/join
            ["(elle-surface 1)"
             "(module \"std/semver\")"
             "(version \"1.0.0\")"
             "(mode :hybrid)"
             "(released :commit \"4f2a9c1e\" :date \"2026-09-21\")"
             "(tests \"tests/elle/semver*.lisp\")"
             "(constructor [])"
             "(export compare :fn [a b] :signals [:error] :doc \"b0a1c2d3\")"
             "(export parse :fn [version] :signals [:error] :doc \"18293a4b\")"
             "(export valid? :fn [version] :signals [] :doc \"90a1b2c3\")"]
            "\n")
          "\n"))
(def surface (sfile:parse surface-text))
(assert (= (surface :version) "1.0.0") "the version form")
(assert (= ((get (surface :exports) :parse) :params) ["version"]) "an export's shape")
(assert (= (sfile:render surface) surface-text) "the render is byte for byte")
```

Vocabulary, per form:

- `(elle-surface 1)` — the format version.
- `(module "std/semver")` — the import spec the surface describes.
- `(version "1.0.0")` — the `(elle/version ...)` the module declared at
  release.
- `(mode :hybrid)` — `:hybrid` when the extractor also loaded the module;
  `:static` when construction signaled and only analysis ran.
- `(released ...)` — provenance: the commit and date of the release.
- `(tests "glob")` — the test files that pin this release's behavior,
  resolved at the released commit for arbitration.
- `(constructor [params])` — the module closure's parameter shape, absent
  for a bare-struct module.
- `(export name :fn [params] ...)` — one export's record.

An export record's parameter shape uses Elle's own syntax: `[a &opt b]`,
`[a & rest]`, `[a &keys opts]`, `[&named :host :port]`. For `&named` the
sorted keys are the contract. Positional names are recorded for
readability only. `:signals` lists the sorted signal bits;
`:propagates [0 2]` appears when the function propagates parameter
signals. `:doc` is a truncated hash of the docstring — present so a doc
change is visible, short so the diff stays one line. A value export is
`(export name :value ...)` with its type and value hash. A trait-carrying
value adds `:traits {:Collection [:conj ...]}` — method names only.

## The floor

A surface diff proves a lower bound on the required bump — the floor.
Behavioral equivalence is undecidable, so the floor is one leg of three:
the diff computes the floor, the prior release's tests arbitrate compat
claims, and majors ship the rules above. The `elle semver` tool runs
all three.

| Surface change | Floor |
|---|---|
| export removed | major |
| export added | minor |
| export kind flip (fn ↔ value) | major |
| required arity increased | major |
| maximum arity decreased | major |
| required parameter became optional | minor |
| new optional parameter or new rest collector | minor |
| rest kind `&named` → `&keys` (strict to open) | minor |
| any other rest-kind change | major |
| `&named` key removed | major |
| `&named` key added | minor |
| positional parameter renamed, shape identical | none |
| signal bit added (including `:yields` appearing) | major |
| signal bit removed | patch |
| propagates index added | major |
| propagates index removed | patch |
| docstring changed | patch |
| value export: type changed | major |
| value export: value hash changed | patch |
| trait protocol or method removed | major |
| trait protocol or method added | minor |
| constructor shape change | as any function |

The floor of a release is the maximum over its changes, ordered
`none < patch < minor < major`.

**Pre-1.0** follows cargo semantics: for a `0.y.z` baseline, a major
floor requires bumping `y`; minor and patch floors bump `z`.

## What versions cannot see

The floor reads declared shape. A function that keeps its shape and
changes its answers moves no floor. That is why a patch or minor claim
is arbitrated by running the previous release's tests against the new
code, and why the arbitration is a gate, not a proof — it only sees what
the old tests exercised.

---

## See also

- [epochs](epochs.md) — the language's own migration system, which this
  design mirrors
- [modules](modules.md) — why the returned struct literal is the public
  surface
