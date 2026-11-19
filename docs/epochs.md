# Epochs

<!-- audited: 2026-11-18 -->

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
[src/epoch/rules.rs](../src/epoch/rules.rs) has ten variants: `Rename`,
`Remove`, `Unwrap`, `Replace`, `MillisToSeconds`, `FlattenBindings`,
`FlattenClauses`, `Desugar`, `DefIntoLet` and `DefsBeforeReads`.
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

### DefIntoLet

Moves a `def`, `var` or `defn` that stands outside a body into a `let`
([bindings](bindings.md)). The rule carries no data, and runs after every rule
in the range but `DefsBeforeReads`.

Migration runs before expansion, so the rule reads only the forms whose shape
it knows without expanding them:

| Where the `def` stands | What takes its place |
|------------------------|----------------------|
| a form after the first operand of `when`, `unless`, `while` or `repeat`, or any form of `forever` | a `let` from the `def` to the end of the form |
| any other operand of a form the rule knows: a branch of `if`, a clause of `cond`, an operand of `and` or `or`, a test, a binding's value | `(let [x v] x)` |
| a `begin` or `do` anywhere but directly in a body the rule knows | a `let` in its place |

Each `def` in a run of them becomes one binding of the same `let`. A `var`
binds its name with `@`, and a `defn` binds its name to a `fn`. The forms after
the run are the body of the `let`, so a `def` among them already stands in a
body. When no form follows the run, the body reads the last name, so the form
keeps its value. The top of a quasiquote template may land in a body, so a
`begin` there stays.

The rule leaves a `def` as written where a `let` could change what a read
finds. That is a read of its name in the body around it, outside the new `let`
and before that body binds the name again. A read before the `let` counts,
because a loop or an earlier function can reach the `def`. So does a read in
the `def`'s own value or in an earlier binding of the same `let`.

The rule also leaves three kinds of `def` it cannot read. One stands directly
in a form the rule does not know, such as a call or a user's macro. One has a
name that a template fills in. One binds a pattern, and no form follows it to
give the value. The compiler then reports each `def` the rule leaves, and `elle
rewrite` names it.

### DefsBeforeReads

Carries a body across epoch 18, which binds a name from its `def` onward
([bindings](bindings.md)). The rule carries no data, and runs after
`DefIntoLet`. It reads the forms of each body that `DefIntoLet` knows.

A forward read is a read of a name inside a lambda, in a form that stands
before the body's first `def` of that name. The rule first moves each `def` a
forward read reaches above the read, where the move cannot change what a form
does. It then binds each forward read that is left in a `letrec`.

A `def` moves only when it runs no code: its value is a function (a `defn`,
or a `fn` form), a literal, a quoted form, or a name. It moves only above
forms that run no code either: such defs, and `defmacro` forms. So no code
that runs can tell that it moved. Up to epoch 17 a call that reached a
function before its `def` failed, and a move would make that call succeed.
The move must also leave every read finding what it found:

- The `def` binds one name, without `@`, and it is the body's only `def` of
  that name.
- It stands directly in the body, not in a `begin`, and it is not the body's
  last form, whose value is the body's value.
- No form it moves above reads its name directly. Up to epoch 17 such a read
  found a binding outside the body.
- Each `def` its value reads that would then stand after it moves too, and a
  forward read in a function it holds counts. A `def` that cannot move keeps
  every `def` that reads it in place.

Defs that read each other move together. The later ones also move up to the
first of them, so the `letrec` they need holds them alone. A moved `def` lands
directly above the first form that reads it, after each moved `def` it reads,
and moved defs otherwise keep the order they were written in. The comment
lines directly above a `def` move with it. Each moved `def` counts as one
rewrite.

A forward read the moves leave, such as one between functions that call each
other or one past a form that runs code, becomes a `letrec`. The rule turns
the run of defs from the reading `def` through the `def` it reads into one
`letrec`, and joins runs that overlap. A `var` binds its name with `@`, and a
`defn` binds its name to a `fn`. The forms after the run are the body of the
`letrec`, and when no form follows, the body reads the last name.

Up to epoch 17, a direct read of a name in its own `def`'s value reads the
binding before that `def`. The rule turns such a `def` into a `let` over the
forms after it, whose value reads that binding too.

The rule leaves a run as written when a `letrec` cannot hold it or would change
what a read finds. That is a run that holds a form other than a `def`, binds a
name twice, or holds a direct read of a name that the same or a later `def` in
it binds. It also leaves a forward read in a form other than a `def`, and a
direct read of a `def`'s own name where a lambda in its value reads the name
too. At epoch 18 a forward read the rule leaves may find an outer binding
instead, so the compiler refuses the file and names the read. `elle rewrite`
names each form the rule leaves.

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
2. Add a `Migration` entry to the `MIGRATIONS` array with the new epoch number,
   a summary, and the rules describing the change. A change to tokenization
   also flips a `Lexicon` field in `Lexicon::for_epoch` and names itself in the
   entry's `lexical` list — see [`impl/lexicon.md`](impl/lexicon.md).
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

[migrations.rs](../src/epoch/rules/migrations.rs) holds every entry.

Files that declare `(elle/epoch 0)` will continue to compile — the compiler
transparently applies the migration rules. Authors can run `elle rewrite`
to update their source and move the epoch tag to the current epoch.

