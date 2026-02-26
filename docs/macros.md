# Macros: Design Document

This document describes Elle's macro system — what exists today and
the remaining work for full hygiene.


## Current State

Macros in Elle are VM-evaluated. A macro is a name, a parameter list,
and a body. At expansion time, arguments are quoted and the body is
compiled and executed in the real VM via `pipeline::eval_syntax()`.
The full language is available in macro bodies: `if`, `let`, closures,
list operations, recursion — everything.

```lisp
(defmacro my-when (test body)
  `(if ,test ,body nil))

(my-when (> x 0) (print "positive"))
;; Expands to: (if (> x 0) (print "positive") nil)
```

### What works

- **VM-evaluated macros.** `defmacro` bodies are normal Elle code.
  Arguments are quoted and bound via `let`. The body runs in the VM
  and must return syntax (typically via quasiquote).

- **Conditional expansion.** Macros can use `if`, `cond`, `let`, etc.
  to generate different code based on their arguments.

- **Threading macros.** `->` and `->>` are built into the expander as
  structural rewrites.

- **Macro introspection.** `macro?` and `expand-macro` work at expansion
  time (handled by the Expander in `expand/introspection.rs`).

- **`define-macro` alias.** Both `defmacro` and `define-macro` are
  accepted. They are identical.

- **Macro definitions expand to nil.** `(defmacro ...)` returns
  `SyntaxKind::Nil` — the definition itself produces no code. This
  matters in `begin` forms where `defmacro` is mixed with expressions.

- **Arity checking.** Wrong argument count produces a clear error.

- **Recursion guard.** Expansion depth is limited to 200 (matching
  Janet), preventing infinite macro expansion.

- **Define shorthand.** `(def (f x) body)` desugars to
  `(def f (fn (x) body))` during expansion.

### Known limitations

**`gensym` is rarely needed.** With automatic hygiene (PR 3), most
macros don't need `gensym`. It's still available for cases where you
need a unique name that's not related to hygiene (e.g., generating
unique global names).

**Macros cannot return improper lists.** `from_value()` requires proper
lists. A macro body that returns `(cons 1 2)` will error.

**REPL macro persistence.** `compile` creates a fresh Expander per call,
so macros defined in one REPL input are lost before the next.


## Architecture

### Pipeline position

```
Source → Reader → Syntax → Expander.expand() → Syntax → Analyzer → HIR
```

Expansion happens between parsing and analysis. The Expander is a
standalone struct with a `HashMap<String, MacroDef>` of registered macros
and a monotonic `ScopeId` counter.

### MacroDef

```rust
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    pub template: Syntax,
    pub definition_scope: ScopeId,
}
```

A macro is a name, positional parameter names, a Syntax template, and a
scope ID. No pattern matching, no ellipsis, no multiple clauses.

### Expansion algorithm (VM-based)

1. Check arity: `args.len() == params.len()`
2. Check recursion depth against `MAX_MACRO_EXPANSION_DEPTH` (200)
3. Build a let-expression: `(let ((p1 'a1) (p2 'a2)) body)` where
   each argument is quoted so it becomes data, not code
4. Compile and execute via `pipeline::eval_syntax()` — the full
   pipeline (expand → analyze → lower → emit → execute) runs on the
   let-expression, using the same Expander (so nested macros work)
5. Convert the result `Value` back to `Syntax` via `from_value()`
6. Stamp a fresh `ScopeId` onto every node in the result via
   `add_scope_recursive()`. **Note:** this stamps ALL nodes, including
   argument-derived nodes. See hygiene plan for the fix.
7. Recursively expand the result (handles macro-generated macro calls)

### Expander precedence

The Expander checks forms in this order: `defmacro`/`define-macro` →
threading macros → `macro?`/`expand-macro` → `define` shorthand →
user-defined macros → recursive child expansion. A user-defined macro
named `define` would never fire because the `define` shorthand is
checked first.

### The scope set mechanism

Every `Syntax` node carries `scopes: Vec<ScopeId>`. The Expander creates
a fresh scope per expansion and stamps it onto the result. This implements
Racket's "sets of scopes" model: two identifiers match only if their
scope sets are compatible.

The Analyzer's `bind()` stores scope sets alongside bindings, and
`lookup()` uses subset matching: a binding is visible to a reference if
the binding's scope set is a subset of the reference's scope set. When
multiple bindings match, the one with the largest scope set wins.

### Syntax objects in the Value system

`Value::syntax(Syntax)` preserves scope sets through the Value round-trip
during macro expansion. Without this, nested macros lose call-site scopes
when arguments pass through `to_value()` → VM execution → `from_value()`.

Macro arguments use hybrid wrapping: atoms (nil, bool, int, float, string,
keyword) are wrapped via `Quote` to preserve runtime semantics. Symbols
and compound forms are wrapped via `SyntaxLiteral(Value::syntax(arg))` to
preserve scope sets. This avoids the problem where wrapping `#f` in a
syntax object makes it truthy (syntax objects are heap-allocated).

### Cross-form macro visibility

`compile_all` shares a single `Expander` across all top-level forms,
so macros defined in one form are visible in subsequent forms within the
same compilation unit. `eval` creates a fresh `Expander` per call,
so macros defined in one REPL input are lost before the next. The REPL
needs to persist the Expander across inputs.


## The Hygiene Problem

Macro hygiene means two things:

1. **No accidental capture.** A binding introduced by a macro doesn't
   shadow bindings at the call site, and vice versa.

2. **Referential transparency.** Free variables in a macro template
   resolve in the macro's definition environment, not the call site.

Without hygiene, macro authors must manually avoid name collisions. The
standard workaround is `gensym` — generating unique names that can't
collide.

### Prior art

**Common Lisp** has `defmacro` with manual `gensym`. No automatic
hygiene. Macro authors are responsible for avoiding capture. This works
in practice because experienced Lispers know the patterns, but it's a
source of subtle bugs.

**Scheme R5RS** has `syntax-rules`, a pattern-based macro system with
automatic hygiene. Patterns use ellipsis (`...`) for variadic matching.
Hygiene is enforced by the expander — no escape hatch. Limited: you
can't write procedural macros.

**Scheme R6RS / Racket** has `syntax-case`, which combines pattern
matching with procedural escape. Macros receive and return *syntax
objects* — s-expressions annotated with lexical context. Hygiene is
automatic but breakable via `datum->syntax`. This is the most powerful
and most complex model.

**Racket's "sets of scopes"** (Matthew Flatt, 2016) replaced the older
"marks and renames" model. Each identifier carries a set of scope IDs.
A binding is visible to a reference if the binding's scope set is a
subset of the reference's scope set. This is what Elle's `Syntax.scopes`
was designed for — the infrastructure exists, the wiring doesn't.


## Implemented: Sets-of-Scopes Hygiene (PR 3)

Binding resolution respects scope marks. Macro-introduced bindings can't
capture call-site names and vice versa. Automatic — no `gensym` needed
for the common case.

**What changed:**

- `Scope.bindings` stores `HashMap<String, Vec<ScopedBinding>>` — multiple
  bindings per name with different scope sets
- `bind()` records the binding's scope set from the `Syntax` node
- `lookup()` uses scope-set subset matching with largest-scope-set-wins
  tiebreaker
- `Value::syntax(Syntax)` preserves scope sets through the Value round-trip
- `SyntaxKind::SyntaxLiteral(Value)` injects syntax objects into the pipeline
- `from_value()` unwraps syntax objects, preserving scopes

**How it works:**

The rule: a binding is visible to a reference if the **binding's** scope
set is a subset of the **reference's** scope set. When multiple bindings
match, the one with the **largest** scope set wins (most specific).

```
Before expansion:
  call-site `tmp` has scopes {0}       (user's let-binding)
  call-site `x` has scopes {0}

After expanding (swap x y) with intro scope 3:
  macro's `tmp` has scopes {0, 3}      ← from result, gets intro scope
  macro's `x` has scopes {0}           ← from call site, no intro scope
```

**Inside the macro body** — reference to `tmp` has scopes `{0, 3}`:
- Call-site binding `tmp` scopes `{0}`: is `{0} ⊆ {0, 3}`? Yes.
- Macro binding `tmp` scopes `{0, 3}`: is `{0, 3} ⊆ {0, 3}`? Yes.
- Both match, but `{0, 3}` is larger → macro's `tmp` wins. Correct.

**At the call site** — reference to `tmp` has scopes `{0}`:
- Call-site binding `tmp` scopes `{0}`: is `{0} ⊆ {0}`? Yes. Matches.
- Macro binding `tmp` scopes `{0, 3}`: is `{0, 3} ⊆ {0}`? No. Invisible.
- Only the call-site `tmp` is visible. No capture.

**Pre-expansion code**: empty scopes `[]` is a subset of everything,
so code that hasn't been through macro expansion works identically.


## What This Unblocks

Features that are now possible with VM-based macros:

| Feature | Defined in | Status |
|---------|-----------|--------|
| `try`/`catch` | `docs/except.md` | Ready to implement |
| `defer` | `docs/janet.md` | Ready to implement |
| `with` | `docs/janet.md` | Ready to implement |
| `protect` | `docs/janet.md` | Ready to implement |
| `generate` | `docs/janet.md` | Ready to implement |
| `bench` | `docs/debugging.md` | Ready to implement |
| `swap` | — | Ready (automatic hygiene via PR 3) |
| Anaphoric macros | — | Implemented via `datum->syntax` |
| `assert` (variadic) | — | Needs variadic macro params |
| `match` (as macro) | — | Ready to implement |


## Files

| File | Role |
|------|------|
| `src/syntax/expand/mod.rs` | Expander struct, `defmacro` handling, scope stamping |
| `src/syntax/expand/macro_expand.rs` | VM-based macro expansion via `eval_syntax` |
| `src/syntax/expand/quasiquote.rs` | Quasiquote → `(list ...)` runtime calls |
| `src/syntax/expand/threading.rs` | `->` and `->>` |
| `src/syntax/expand/introspection.rs` | `macro?` and `expand-macro` |
| `src/syntax/expand/qualified.rs` | `module:name` resolution |
| `src/syntax/expand/tests.rs` | Expansion tests |
| `src/syntax/mod.rs` | `Syntax`, `SyntaxKind`, `ScopeId`, `set_scopes_recursive` |
| `src/syntax/convert.rs` | `Syntax` ↔ `Value` conversion |
| `src/hir/analyze/mod.rs` | `Analyzer`, `Scope`, `lookup()`, `bind()` |
| `src/pipeline.rs` | Compilation entry points, `eval_syntax` |


## Resolved Questions

These were open during design; now answered by the implementation:

1. **Argument quoting.** `Quote(Box::new(arg.clone()))` works. The
   Analyzer handles `quote` by converting to a Value via `to_value()`.
   Symbols inside quotes are interned, not resolved.

2. **Analysis-only paths.** Both `lsp/` and `lint/cli` already
    create VMs. `analyze`/`analyze_all` take `&mut VM`.

3. **Effect system interaction.** Effect inference happens after
   expansion, so macros that expand to effectful code get correct
   effect annotations. No changes needed.


## Hygiene Escape Hatch: `datum->syntax`

`(datum->syntax context datum)` creates a syntax object from `datum`
with the lexical context of `context`. The result is marked
`scope_exempt` so the expansion pipeline's intro scope stamping does
not override the context's scopes. This enables anaphoric macros —
macros that intentionally introduce bindings visible at the call site.

```lisp
(defmacro aif (test then else)
  `(let ((,(datum->syntax test 'it) ,test))
     (if ,(datum->syntax test 'it) ,then ,else)))

(aif (+ 1 2) (+ it 10) 0)  ;; → 13
```

If `context` is a syntax object, its scope set and span are copied.
If `context` is a plain value (atom arguments are passed as plain
values via hybrid wrapping), empty scopes and a synthetic span are
used — normal lexical scoping still applies.

`(syntax->datum stx)` strips scope information from a syntax object,
returning the plain value. If the argument is not a syntax object, it
is returned unchanged.

### Implementation

Both are runtime primitives in `src/primitives/meta.rs`. They access
the symbol table via the thread-local `get_symbol_table()` pattern.

The `scope_exempt: bool` field on `Syntax` is the mechanism that
prevents intro scope stamping. `add_scope_recursive` checks this flag
and skips exempt nodes. `set_scopes_recursive` (called by
`datum->syntax`) sets both the scopes and the exempt flag recursively.


## Open Questions

1. **Performance.** Every macro call compiles and executes bytecode.
   For hot macros (e.g., `when` used hundreds of times), this could be
   slow. Mitigation: cache compiled bytecode per MacroDef. The body
   doesn't change between calls — only the argument bindings do.

2. **Interaction between `set!` and scope-aware lookup.** `set!` goes
   through the Analyzer's `lookup()`. With scope-aware resolution, a
   macro that uses `set!` on a call-site variable must have the right
   scope set for the reference to resolve. This works naturally because
   call-site arguments keep their original scopes via syntax objects.
   The `swap` macro's `set!` on call-site variables is tested and works.
