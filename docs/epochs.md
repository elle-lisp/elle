# Epochs

Epochs are Elle's mechanism for making breaking changes to the language while
preserving backwards compatibility. Each epoch is a numbered version of the
language surface syntax. Source files can declare the epoch they target, and
the compiler will transparently rewrite old-epoch syntax before compilation.

## Declaring an epoch

Place `(elle/epoch N)` as the first form in a source file:

```lisp
(elle/epoch 0)
(def x 10)
(println x)
```

The `(elle/epoch N)` declaration tells the compiler which epoch the file was written
for. It is consumed during compilation and does not appear in the running
program. Files without an epoch declaration target the current epoch.

To query the current epoch at runtime:

```lisp
(elle/epoch)   # returns the current epoch number
```

## What happens at compile time

The epoch migration pass runs after parsing and before macro expansion:

```
Source → Reader → [Epoch Migration] → Expander → HIR → LIR → Bytecode
```

If a file declares `(elle/epoch N)` where N is older than the current epoch, the
compiler applies all migration rules from epoch N+1 through the current epoch
to the parsed syntax tree. This is transparent — old-epoch code compiles and
runs exactly as if it had been written using current-epoch syntax.

## Migration rule types

Each epoch bump defines a set of migration rules. There are three types:

### Rename

Mechanically renames a symbol everywhere it appears (except inside quotes):

```rust
MigrationRule::Rename { old: "old-name", new: "new-name" }
```

All occurrences of `old-name` become `new-name`. Renames chain across epochs:
if epoch 1 renames A→B and epoch 2 renames B→C, a file at epoch 0 sees A→C.

### Replace

Structurally rewrites a call form by matching the head symbol and argument
count, then substituting into a template:

```rust
MigrationRule::Replace {
    symbol: "assert-eq",
    arity: 3,
    template: "(assert (= $1 $2) $3)",
}
```

This rewrites `(assert-eq X Y msg)` to `(assert (= X Y) msg)`. Placeholders
`$1`, `$2`, ... refer to arguments by position (1-indexed, after the head
symbol). The template uses current-epoch syntax. Arguments are spliced in as
complete subtrees.

If the arity does not match, the form is left unchanged — this allows a symbol
to be used with different arities without triggering an unintended rewrite.

### Remove

Flags a form as removed. Any use of the symbol produces a compile error with
a message telling the author what to use instead:

```rust
MigrationRule::Remove {
    symbol: "old-fn",
    message: "use (new-fn ...) instead",
}
```

Removals require the author to manually update the code. They are also checked
by `elle rewrite` (see below).

## The `elle rewrite` CLI tool

`elle rewrite` is a source-to-source migration tool that updates files in
place. It applies the same rules as the compiler but modifies the source text
directly (preserving comments, whitespace, and formatting) and updates the
`(elle/epoch N)` tag to the current epoch.

```
elle rewrite [OPTIONS] <file...>
```

**Options:**
- `--check` — Report files that need changes (exit 1 if any). Does not modify files.
- `--dry-run` — Show what would change without writing.
- `--list-rules` — Print all migration rules for the current epoch.

**Example workflow:**

```sh
# See what would change
elle rewrite --dry-run tests/*.lisp

# Apply rewrites
elle rewrite tests/*.lisp

# Verify in CI that all files are up to date
elle rewrite --check tests/*.lisp
```

After rewriting, the file's `(elle/epoch N)` tag is updated to the current epoch.
Files without an epoch tag get one added.

## Adding a new migration

To make a breaking change to Elle:

1. Bump `CURRENT_EPOCH` in `src/epoch/rules.rs`.
2. Add a `Migration` entry to the `MIGRATIONS` array with the new epoch number,
   a summary, and the rules describing the change.
3. Update `(elle/epoch N)` in `stdlib.lisp` to the new epoch. The WASM backend
   strips stdlib's epoch tag and concatenates the body as-is — it does not run
   migration on stdlib, so stdlib must already use current-epoch syntax.
4. Add tests in `src/epoch/transform.rs` and `src/rewrite/run.rs`.
5. Run `make smoke` to verify the full test suite still passes.

Example:

```rust
Migration {
    epoch: 1,
    summary: "consolidate assertion forms",
    rules: &[
        MigrationRule::Rename { old: "assert-true", new: "assert" },
        MigrationRule::Replace {
            symbol: "assert-eq",
            arity: 3,
            template: "(assert (= $1 $2) $3)",
        },
        MigrationRule::Remove {
            symbol: "assert-err",
            message: "use (let [[ok? _] (protect (f))] (assert (not ok?) msg)) instead",
        },
    ],
},
```

Files that declare `(elle/epoch 0)` will continue to compile — the compiler
transparently applies the migration rules. Authors can run `elle rewrite`
to update their source and remove the epoch tag.

## Epoch history

### Epoch 1 — consolidate assertion helpers

Replaced `assert-true`, `assert-false`, `assert-eq`, `assert-equal`,
`assert-string-eq`, `assert-list-eq`, `assert-not-nil`, `assert-err`, and
`assert-err-kind` with the single `(assert expr msg)` form.

### Epoch 2 — print→println, newline→println, drop write

- `print` renamed to `println` (output with trailing newline).
- `newline` renamed to `println` (zero-arg newline is now `(println)`).
- `write` removed — use `(pp ...)` for literal form or `(port/write port data)` for port I/O.

### Epoch 3 — display→print

- `display` renamed to `print` (output without trailing newline).

After epoch 3, the output API is:

| Function   | Target   | Newline? |
|------------|----------|----------|
| `print`    | `*stdout*` | no     |
| `println`  | `*stdout*` | yes    |
| `eprint`   | `*stderr*` | no     |
| `eprintln` | `*stderr*` | yes    |

All four respect `parameterize` rebinding of `*stdout*`/`*stderr*`.

### Epoch 4 — stream/{read,read-line,read-all,write,flush} → port/...

Port I/O primitives moved from the `stream/` namespace to `port/`:

| Old name | New name |
|----------|----------|
| `stream/read-line` | `port/read-line` |
| `stream/read` | `port/read` |
| `stream/read-all` | `port/read-all` |
| `stream/write` | `port/write` |
| `stream/flush` | `port/flush` |

These five operations act exclusively on ports, not on abstract streams.
The `stream/` namespace now contains only stream combinators (`stream/map`,
`stream/filter`, `stream/collect`, etc.) which operate on lazy sequences.
The old `stream/` names remain as aliases.

### Epoch 5 — polymorphic `has?`/`put`, retire string-specific containment

- `has?` is now the canonical membership predicate for structs, sets, and
  strings. `contains?` remains as a permanent alias.
- `string-contains?` renamed to `has?`.
- `string/contains?` renamed to `has?`.
- `put` now accepts 2 arguments for sets: `(put set value)`. The set-specific
  `add` is rewritten to `put` by `elle rewrite`.

### Epoch 6 — remove ev/run from user code

User code already runs in the async scheduler. `(ev/run (fn [] body...))`
is unwrapped to just `body...`. Non-lambda forms produce a compile error.

### Epoch 7 — flat let bindings

`let`, `let*`, `letrec`, `if-let`, and `when-let` switch from nested-pair
bindings to flat (Clojure-style) bindings. Each binding is a pattern/value
pair laid out flat inside a single bracket form.

| Old (epoch ≤ 6) | New (epoch 7) |
|-----------------|---------------|
| `(let [[a 1] [b 2]] ...)` | `(let [a 1 b 2] ...)` |
| `(let [[[x y] (foo)]] ...)` | `(let [[x y] (foo)] ...)` |
| `(let* [[a 1] [b (+ a 1)]] ...)` | `(let* [a 1 b (+ a 1)] ...)` |
| `(if-let [[x (find-it)]] ...)` | `(if-let [x (find-it)] ...)` |

Destructuring is unambiguous: each form occupies exactly one syntactic
position, alternating pattern then value. `elle rewrite` handles the
migration mechanically.

### Epoch 8 — immutable-by-default bindings

All bindings (`def`, `let`, `let*`, `letrec`, function parameters) are
immutable by default. `var` is replaced by `def @`, and mutable bindings
use the `@` prefix:

| Old (epoch ≤ 7) | New (epoch 8) |
|-----------------|---------------|
| `(var x 0)` | `(def @x 0)` |
| `(let [x 0] (assign x 1) ...)` | `(let [@x 0] (assign x 1) ...)` |
| `(defn f [n] (assign n 10))` | `(defn f [@n] (assign n 10))` |

The `@` prefix appears only at the binding site. All subsequent references
omit it. Assigning to a binding without `@` is a compile-time error:

```
cannot assign immutable binding 'x' (use @x to make it mutable)
```

Immutable bindings enable constant propagation by the compiler and
eliminate the need for cell indirection when captured by closures.

`elle rewrite` handles the `var → def @` migration. Mutable `let` and
parameter bindings require manual `@` annotation, guided by compile errors.

### Epoch 9 — flat cond/match clauses

`cond` and `match` clauses switch from parenthesized groups to flat pairs:

| Old (epoch ≤ 8) | New (epoch 9) |
|-----------------|---------------|
| `(cond (test1 body1) (test2 body2))` | `(cond test1 body1 test2 body2)` |
| `(match val (pat1 body1) (pat2 body2))` | `(match val pat1 body1 pat2 body2)` |

Multi-body arms are wrapped in `(begin ...)`. The `(else body)` form in
`cond` becomes a trailing default expression.

### Epoch 10 — cons→pair, car→first, cdr→rest

Classic Lisp pair操作 names are replaced with descriptive alternatives:

| Old (epoch ≤ 9) | New (epoch 10) |
|-----------------|----------------|
| `cons` | `pair` |
| `car` | `first` |
| `cdr` | `rest` |
