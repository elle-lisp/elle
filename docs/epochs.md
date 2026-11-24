# Epochs

<!-- audited: 2026-11-24 -->

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

Epochs version the syntax. The peer declaration `(unicode! N)` versions the
string semantics: it selects the Unicode segmentation generation the program
runs under. Where `(elle/epoch N)` is consumed and rewrites the tree,
`(unicode! N)` is checked by the analyzer and locks the table generation.
See [`compile-time.md`](compile-time.md) and [`strings.md`](strings.md).

## What happens at compile time

The epoch migration pass runs after parsing and before macro expansion:

```
Source → [epoch prescan] → Reader → [Epoch Migration] → Expander → HIR → LIR
       → Bytecode
```

If a file declares `(elle/epoch N)` where N is older than the current epoch, the
compiler applies all migration rules from epoch N+1 through the current epoch
to the parsed syntax tree. This is transparent — old-epoch code compiles and
runs exactly as if it had been written using current-epoch syntax.

Because migration operates on parsed trees, it cannot express changes to
tokenization itself. The reader therefore reads the declaration before it
lexes, with a frozen micro-grammar, and tokenizes under the lexer rules that
epoch selects — see [`impl/lexicon.md`](impl/lexicon.md). Epoch 13 is the
first epoch whose lexer rules differ from its predecessor's: it changes what a
string escape means.

## Migration rule types

Each epoch bump defines a set of migration rules. The `MigrationRule` enum in
[src/epoch/rules.rs](../src/epoch/rules.rs) has eleven variants: `Rename`,
`Remove`, `Unwrap`, `Replace`, `MillisToSeconds`, `FlattenBindings`,
`FlattenClauses`, `Desugar`, `FoldModule`, `DefIntoLet` and
`DefsBeforeReads`.
`elle rewrite --list-rules` lists the rules each epoch adds, under that
epoch's summary.

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
to be used with different arities without triggering an unintended rewrite. A
symbol may carry one rule for each arity, as `silence` does in epoch 15.

A matched form is rewritten once. The walk then descends into the elements of
the replacement. So a template may keep its own head symbol, as epoch 15's
`(squelch $2 $1)` does, and a form nested in an argument is rewritten as well.
`elle rewrite` builds the replacement from the arguments' rewritten text, so it
reaches the same nested forms.

### MillisToSeconds

Rewrites a duration a call gave in milliseconds as a `:timeout` in seconds. The
rule names the calls and where the duration sits:

```rust
MigrationRule::MillisToSeconds {
    symbols: &["port/read", "port/write"],
    arg: TimeArg::Keyword,
}
MigrationRule::MillisToSeconds {
    symbols: &["sys/join", "os/join"],
    arg: TimeArg::Last { arity: 2, negative_unbounded: false },
}
```

`Keyword` rewrites the value after `:timeout`. `Last` matches a call with
exactly `arity` arguments and replaces the last one with `:timeout` and its
value, so `(sys/join h 500)` becomes `(sys/join h :timeout 0.5)`. The value is
rewritten by its shape:

| Milliseconds | Seconds |
|---|---|
| a literal, `500` or `5000` | the literal, `0.5` or `5` |
| `nil` | `nil` after a keyword; no argument at all for `Last` |
| a negative literal, when `negative_unbounded` | no argument at all |
| any other expression `e` | `(if-let [ms e] (/ ms 1000.0) nil)` |

A `negative_unbounded` rule wraps `e` so that a negative value becomes `nil`,
since the call read a negative duration as no bound. A head that an older epoch
renamed is matched by its new name as well as its old one.

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

### Desugar

Replaces a reader shorthand with the form it stands for. A shorthand is a
prefix token that wraps the form written after it — `;expr` stands for
`(splice expr)`:

```rust
MigrationRule::Desugar { shorthand: Token::Splice }
```

The rule names only the token. `Token::shorthand_form` says which form each
shorthand stands for, so the rule cannot name the wrong one.

An epoch declares this when it takes the shorthand's spelling away, which
makes the change lexical as well as structural. The rule is the half that
`Lexicon::respell` cannot reach: a respelling answers about one token, and
this rewrite has to reach past the prefix to the end of the form the prefix
wraps. See [`impl/lexicon.md`](impl/lexicon.md) for the mechanism and for
the condition a shorthand must meet before an epoch may desugar it.

### FoldModule

Carries a module across epoch 16, which makes a module one form
([modules](modules.md)). The rule carries no data. It runs after the table
rules and before `DefIntoLet`, and it reads a file's top-level forms alone.

The rule folds a file that holds more than one form and whose last form is a
`fn` form. Every form before the function moves into the function's body,
ahead of the body's own forms, and the function becomes the file's one form.
A string that opens a body of more than one form stays first, as the
function's docstring:

```lisp
(def old-module "(elle/epoch 15)
                 (def greeting \"Hi\")
                 (defn hi [n] (string greeting \" \" n))
                 (fn [] {:hi hi})")
(assert (= (map syntax/->datum (compile/read-forms old-module "<doc>" :module))
           '((fn []
               (def greeting "Hi")
               (defn hi [n] (string greeting " " n))
               {:hi hi})))
        "the forms before the function move into its body")
```

A form before the function may spell a name that a parameter of the function
binds. Moving that form into the body would change what the name means there,
so the rule wraps the forms instead. The function `(fn PARAMS body…)` becomes
`(fn [& args] form… (apply (fn PARAMS body…) args))`, where `args` is a name
that no form of the file spells.

The moved forms run on each call of the function, where they ran once per
load before. An importer that calls the function once per import, as
`((import "x"))` does, sees no difference.

A loader reads a module with `(compile/read-forms source name :module)`, and
only that read folds. A program that `elle` runs and an included file are not
modules. `elle rewrite` cannot tell a module from a program, so it folds every
file whose last form is a `fn` form. A program has no use for a last form that
only builds a function.

### DefIntoLet and DefsBeforeReads

Epoch 17's `DefIntoLet` moves a `def` that stands outside a body into a `let`.
Epoch 18's `DefsBeforeReads` moves a `def` above a function that reads it
early, or binds the two in a `letrec`. Both carry no data, and both
read each body of the tree, so they run last. [bodies.md](epochs/bodies.md)
states where each moves a `def`, and which forms each leaves for their author.

## The `elle rewrite` CLI tool

`elle rewrite` is a source-to-source migration tool that updates files in
place. It applies the same rules as the compiler but modifies the source text
directly (preserving comments, whitespace, and formatting) and updates the
`(elle/epoch N)` tag to the current epoch.

```
elle rewrite [OPTIONS] <file...>
```

It reads each file under the lexicon that file's own epoch selects, so a file
written before a token-level change still tokenizes the way its author meant
(see [`impl/lexicon.md`](impl/lexicon.md)). Tokens whose spelling changed are
rewritten in place alongside the tree rules.

**Options:**
- `--check` — Report files that need changes (exit 1 if any). Does not modify files.
- `--dry-run` — Show what would change without writing.
- `--list-rules` — Print every migration rule, tree rules and lexical changes
  alike, under the epoch that adds it and that epoch's summary.

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
Files without an epoch tag get one added. A form a rule leaves for its author
is named on standard error, by file, line and column.

## Adding a new migration

To make a breaking change to Elle:

1. Bump `CURRENT_EPOCH` in [src/epoch/rules.rs](../src/epoch/rules.rs).
2. Add a `Migration` entry to the end of the `LATER` table with the new epoch
   number, a summary, and the rules describing the change. A change to
   tokenization also flips a `Lexicon` field in `Lexicon::for_epoch` and names
   itself in the entry's `lexical` list — see [`impl/lexicon.md`](impl/lexicon.md).
3. Update `(elle/epoch N)` in [the standard library](../src/stdlib.lisp) to
   the new epoch. The WASM backend strips stdlib's epoch tag and concatenates
   the body as-is — it does not run migration on stdlib, so stdlib must
   already use current-epoch syntax.
4. Add a test file for the epoch under [the tree migration's
   tests](../src/epoch/transform/tests.rs) and [the rewrite's
   tests](../src/rewrite/run/tests.rs).
5. State the new behavior in the document of the feature it changes, and say
   there what a file of an older epoch reads instead.
6. Run `make smoke` to verify the full test suite still passes.

[migrations.rs](../src/epoch/rules/migrations.rs) holds the entries from
epoch 13 on, and [early.rs](../src/epoch/rules/migrations/early.rs) those of
epochs 1 to 12.

Files that declare `(elle/epoch 0)` will continue to compile — the compiler
transparently applies the migration rules. Authors can run `elle rewrite`
to update their source and move the epoch tag to the current epoch.

