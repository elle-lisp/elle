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

- **Define shorthand.** `(define (f x) body)` desugars to
  `(define f (fn (x) body))` during expansion.

### Known limitations

**No hygiene.** The Expander stamps scope marks onto expansion results,
but the Analyzer ignores them. Binding resolution is pure string matching.
A macro that introduces a binding named `tmp` will shadow the caller's
`tmp`. See PR 3 plan below.

**`gensym` returns a string, not a symbol.** Using gensym in quasiquote
templates produces string literals where symbols are needed. See #306.

**`from_value` strips scope sets.** The Value round-trip during macro
expansion loses scope marks from the original arguments. PR 3 must
address this.

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
a fresh scope per expansion and stamps it onto the result. The intent is
Racket's "sets of scopes" model: two identifiers match only if their
scope sets are compatible.

**The problem:** The Analyzer's `lookup()` method walks a `Vec<Scope>`
stack and matches by string name. It never examines `Syntax.scopes`. The
scope marks are dead data.

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


## Remaining Work

### PR 3: Sets-of-scopes hygiene

**Goal:** Binding resolution respects scope marks. Macro-introduced
bindings can't capture call-site names and vice versa. Automatic —
no `gensym` needed for the common case.

**What changes:**

- The Analyzer's `Scope` struct stores scope sets alongside binding names
- `lookup()` uses scope-set subset matching instead of string matching
- `bind()` records the binding's scope set from the `Syntax` node
- The intro scope stamped by `expand_macro_call` (already done in PR 2)
  now has teeth — the Analyzer uses it

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
| `try`/`catch` | `docs/EXCEPT.md` | Ready to implement |
| `defer` | `docs/JANET.md` | Ready (needs gensym fix #306) |
| `with` | `docs/JANET.md` | Ready (needs gensym fix #306) |
| `protect` | `docs/JANET.md` | Ready (needs gensym fix #306) |
| `generate` | `docs/JANET.md` | Ready to implement |
| `bench` | `docs/DEBUGGING.md` | Ready to implement |
| `swap` | — | Needs gensym fix #306 or PR 3 (automatic hygiene) |
| Anaphoric macros | — | Needs PR 3 (hygiene escape) |
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
| `src/syntax/mod.rs` | `Syntax`, `SyntaxKind`, `ScopeId` |
| `src/syntax/convert.rs` | `Syntax` ↔ `Value` conversion |
| `src/hir/analyze/mod.rs` | `Analyzer`, `Scope`, `lookup()`, `bind()` |
| `src/pipeline.rs` | Compilation entry points, `eval_syntax` |


## Resolved Questions

These were open during design; now answered by the implementation:

1. **Argument quoting.** `Quote(Box::new(arg.clone()))` works. The
   Analyzer handles `quote` by converting to a Value via `to_value()`.
   Symbols inside quotes are interned, not resolved.

2. **Analysis-only paths.** Both `elle-lsp` and `elle-lint` already
   create VMs. `analyze`/`analyze_all` take `&mut VM`.

3. **Effect system interaction.** Effect inference happens after
   expansion, so macros that expand to effectful code get correct
   effect annotations. No changes needed.


## Open Questions

1. **Performance.** Every macro call compiles and executes bytecode.
   For hot macros (e.g., `when` used hundreds of times), this could be
   slow. Mitigation: cache compiled bytecode per MacroDef. The body
   doesn't change between calls — only the argument bindings do.

2. **Interaction between `set!` and scope-aware lookup.** `set!` goes
   through the Analyzer's `lookup()`. With scope-aware resolution, a
   macro that uses `set!` on a call-site variable must have the right
   scope set for the reference to resolve. This should work naturally
   (call-site arguments keep their original scopes) but needs careful
   testing, especially for mutable captures across closure boundaries.

3. **Scope representation in the Analyzer.** The current
   `HashMap<String, BindingId>` is fast for string lookup. Scope-aware
   lookup needs to find all bindings with a given name and then pick the
   best match. A `HashMap<String, Vec<(Vec<ScopeId>, BindingId)>>` would
   work. Profile before optimizing.
