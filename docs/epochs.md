# Epochs

Epochs are Elle's mechanism for making breaking changes to the language while
preserving backwards compatibility. Each epoch is a numbered version of the
language surface syntax. Source files can declare the epoch they target, and
the compiler will transparently rewrite old-epoch syntax before compilation.

## Declaring an epoch

Place `(elle N)` as the first form in a source file:

```lisp
(elle 0)
(def x 10)
(display x)
```

The `(elle N)` declaration tells the compiler which epoch the file was written
for. It is consumed during compilation and does not appear in the running
program. Files without an epoch declaration target the current epoch.

## What happens at compile time

The epoch migration pass runs after parsing and before macro expansion:

```
Source → Reader → [Epoch Migration] → Expander → HIR → LIR → Bytecode
```

If a file declares `(elle N)` where N is older than the current epoch, the
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
directly (preserving comments, whitespace, and formatting) and strips the
`(elle N)` tag when done.

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

After rewriting, the file targets the current epoch and the `(elle N)` tag is
removed.

## Adding a new migration

To make a breaking change to Elle:

1. Bump `CURRENT_EPOCH` in `src/epoch/rules.rs`.
2. Add a `Migration` entry to the `MIGRATIONS` array with the new epoch number,
   a summary, and the rules describing the change.
3. Add tests in `src/epoch/transform.rs` and `src/rewrite/run.rs`.
4. Run `make smoke` to verify the full test suite still passes.

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
            message: "use (let (([ok? _] (protect (f)))) (assert (not ok?) msg)) instead",
        },
    ],
},
```

Files that declare `(elle 0)` will continue to compile — the compiler
transparently applies the migration rules. Authors can run `elle rewrite`
to update their source and remove the epoch tag.
