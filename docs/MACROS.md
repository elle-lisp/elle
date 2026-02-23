# Macros: Design Document

This document describes Elle's macro system — what exists today, what's
broken, and the plan to fix it. It records the reasoning and trade-offs
so future readers can understand the design space and pick up where we
left off.


## Current State

Macros in Elle are template-based. A macro is a name, a parameter list,
and a syntax template. Expansion substitutes arguments into the template
at the AST level.

```lisp
(defmacro my-when (test body)
  `(if ,test ,body nil))

(my-when (> x 0) (print "positive"))
;; Expands to: (if (> x 0) (print "positive") nil)
```

### What works

- **Template macros.** `defmacro` with quasiquote templates. Parameters
  are substituted, the result is recursively expanded.

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

- **Define shorthand.** `(define (f x) body)` desugars to
  `(define f (fn (x) body))` during expansion.

### What's broken

**No hygiene.** The Expander stamps scope marks onto expansion results,
but the Analyzer ignores them. Binding resolution is pure string matching.
A macro that introduces a binding named `tmp` will shadow the caller's
`tmp`:

```lisp
(defmacro broken-swap (a b)
  `(let ((tmp ,a)) (set! ,a ,b) (set! ,b tmp)))

(let ((tmp 10) (x 1) (y 2))
  (broken-swap x y)
  tmp)  ; BUG: tmp is now 1, not 10
```

**No compile-time evaluation.** `gensym` is a runtime primitive. It
cannot be called during expansion. This blocks every macro that needs
fresh bindings — which is most useful macros. Documented as broken in
`docs/DEBUGGING.md`.

**Dead infrastructure.** `Syntax.scopes` is written but never read.
`MacroDef.definition_scope` is always `ScopeId(0)`. (The parallel
`MacroDef` in `symbol.rs`, `SymbolTable.macros`, `gensym_id()`, and
the runtime `prim_is_macro`/`prim_expand_macro` stubs were removed in
PR 1.)


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

### Expansion algorithm

1. Check arity: `args.len() == params.len()`
2. Substitute: walk the template via `substitute()`, replacing symbols
   matching parameter names with the corresponding argument syntax trees.
   Quasiquote internals use a separate `substitute_quasiquote()` path
   that only substitutes inside unquote/unquote-splicing nodes.
3. If the *substituted* result is a quasiquote, resolve it via
   `eval_quasiquote_to_syntax()` — collapses unquote nodes in-place
4. Stamp a fresh `ScopeId` onto every node in the result via
   `add_scope_recursive()`. **Bug:** this stamps ALL nodes, including
   substituted call-site arguments. See Tier 2 for the fix.
5. Recursively expand the result (handles macro-generated macro calls)

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

`compile_all_new` shares a single `Expander` across all top-level forms,
so macros defined in one form are visible in subsequent forms within the
same compilation unit. `eval_new` creates a fresh `Expander` per call,
so macros defined in one REPL input are lost before the next. The REPL
needs to persist the Expander across inputs — this is orthogonal to
hygiene but should be fixed alongside Tier 1.


## The Hygiene Problem

Macro hygiene means two things:

1. **No accidental capture.** A binding introduced by a macro doesn't
   shadow bindings at the call site, and vice versa.

2. **Referential transparency.** Free variables in a macro template
   resolve in the macro's definition environment, not the call site.

Without hygiene, macro authors must manually avoid name collisions. The
standard workaround is `gensym` — generating unique names that can't
collide. But Elle's `gensym` is runtime-only, so even this workaround
is unavailable.

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

### What Elle needs

Elle's fiber-based control flow depends on macros. `try`/`catch`,
`defer`, `with`, `protect`, `generate` — all are designed as macros over
`fiber/new` + `resume` + `fiber/status`. The fiber primitives work today.
The sugar is what's missing, and the sugar requires macros that can
introduce fresh bindings.


## Plan: Three Tiers

Each tier is independently shippable and useful. Each builds on the
previous.

### Tier 1: Compile-time gensym

**Goal:** Macros can generate fresh symbol names during expansion.
Common Lisp style — manual hygiene via `gensym`.

**What changes:**

- The Expander gets a `gensym` method that produces unique symbol names
  (e.g., `__Gtmp0`, `__Gtmp1`)
- `(gensym)` and `(gensym "prefix")` are recognized during expansion
  and evaluated at compile time
- Works inside quasiquote templates via unquote: `` `(let ((,(gensym "tmp") ,expr)) ...) ``

**What this unblocks:**

```lisp
(defmacro swap (a b)
  (let ((tmp (gensym "tmp")))
    `(let ((,tmp ,a)) (set! ,a ,b) (set! ,b ,tmp))))

;; Simplified two-argument form. The final try/catch syntax
;; (see docs/EXCEPT.md) will use a different calling convention
;; with inline handler bodies instead of handler functions.
(defmacro try (body handler)
  (let ((f (gensym "f"))
        (result (gensym "result")))
    `(let ((,f (fiber/new (fn () ,body) 1)))
       (fiber/resume ,f)
       (if (= (fiber/status ,f) :error)
         (let ((,result (fiber/value ,f)))
           (,handler ,result))
         (fiber/value ,f)))))
```

**What this does NOT do:** No automatic hygiene. Macro authors who forget
`gensym` still get capture bugs. Free variables in templates still
resolve at the call site.

**Cleanup (done in PR 1):** Removed `symbol.rs::MacroDef`,
`SymbolTable.macros`, `gensym_id()`, `primitives/macros.rs` (entire
file), and the runtime `prim_is_macro`/`prim_expand_macro` stubs from
both `meta.rs` and `registration.rs`. Runtime `prim_gensym` in
`primitives/meta.rs` is kept — it produces strings at runtime and is
used by existing code.

### Tier 2: Sets-of-scopes hygiene

**Goal:** Binding resolution respects scope marks. Macro-introduced
bindings can't capture call-site names and vice versa. Automatic —
no `gensym` needed for the common case.

**What changes:**

- The Analyzer's `Scope` struct stores scope sets alongside binding names
- `lookup()` uses scope-set subset matching instead of string matching
- `bind()` records the binding's scope set from the `Syntax` node
- The Expander's scope stamping is fixed: arguments from the call site
  do NOT get the intro scope (only template-originated nodes do)
- `MacroDef.definition_scope` is set correctly and used for referential
  transparency

**How it works:**

The rule: a binding is visible to a reference if the **binding's** scope
set is a subset of the **reference's** scope set. When multiple bindings
match, the one with the **largest** scope set wins (most specific).

```
Before expansion:
  call-site `tmp` has scopes {0}       (user's let-binding)
  call-site `x` has scopes {0}

After expanding (swap x y) with intro scope 3:
  macro's `tmp` has scopes {0, 3}      ← from template, gets intro scope
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

**What this does NOT do:** No procedural macros. Templates are still
pure substitution — you can't run arbitrary code during expansion.

### Tier 3: Procedural macros

**Goal:** Macros can run arbitrary Elle code during expansion. Computed
templates, conditional expansion, recursive macro helpers.

**What changes:**

- A `SyntaxEvaluator` (tree-walking interpreter over `Syntax`) is added
  to the Expander
- `defmacro` bodies are evaluated by the `SyntaxEvaluator` with
  parameters bound to argument syntax trees
- The body returns a `Syntax` tree that replaces the macro call
- Syntax manipulation primitives: `syntax->datum`, `datum->syntax`,
  `syntax-e`, `with-syntax-scopes`

**Design choice:** The evaluator operates on `Syntax` objects, not
`Value`. This preserves scope annotations from Tier 2, so hygiene
composes with procedural power. If we used plain lists (like Common
Lisp's `defmacro`), we'd lose scope information.

**The evaluator supports:** Literals, `if`, `let`, `begin`, `fn`
(closures over Syntax), function application, `cons`/`car`/`cdr`/
`list`/`append`, `gensym`, `quote`, `quasiquote`/`unquote`.

**What this unblocks:**

```lisp
;; Macro that generates different code based on argument count.
;; Note: variadic macro parameters (. args) require extending MacroDef
;; to support rest-params — this is a Tier 3 prerequisite.
(defmacro assert (test . args)
  (if (empty? args)
    `(if (not ,test) (error "assertion failed"))
    `(if (not ,test) (error ,(first args)))))

;; Macro that processes a list of bindings
(defmacro let-values (bindings body)
  (fold (fn (acc binding)
          `(let ((,(first binding) ,(second binding))) ,acc))
        body
        (reverse bindings)))
```

**Error handling:** When a procedural macro body errors (type mismatch,
unbound variable, etc.), the `SyntaxEvaluator` returns
`Result<Syntax, String>` with source location from the macro definition.
Expansion errors should be clearly distinguishable from analysis errors.

**Limitation:** The evaluator is a separate mini-interpreter. It can't
call runtime functions — only built-in syntax operations and functions
defined within the macro body. This is a fundamental limitation of
tree-walking evaluation at compile time. A future upgrade could compile
macro bodies through the full pipeline (Option B) or use phase
separation (Option C, Racket's model).


## Execution Order

1. **Cleanup** — remove dead code, reduce confusion
2. **Tier 1** — compile-time gensym, unblock `try`/`catch` and friends
3. **Tier 2** — sets-of-scopes, make hygiene automatic
4. **Tier 3** — procedural macros, full power

Each tier is a separate branch/PR. Each leaves the system in a working
state with all tests passing.


## What This Unblocks

These features are designed but blocked on the macro system:

| Feature | Defined in | Needs |
|---------|-----------|-------|
| `try`/`catch` | `docs/EXCEPT.md` | Tier 1 (gensym) |
| `defer` | `docs/JANET.md` | Tier 1 (gensym) |
| `with` | `docs/JANET.md` | Tier 1 (gensym) |
| `protect` | `docs/JANET.md` | Tier 1 (gensym) |
| `generate` | `docs/JANET.md` | Tier 1 (gensym) |
| `bench` | `docs/DEBUGGING.md` | Tier 1 (gensym) |
| `swap` | — | Tier 1 (gensym) or Tier 2 (automatic) |
| Anaphoric macros | — | Tier 3 (`datum->syntax`) |
| `assert` (variadic) | — | Tier 3 (procedural) |
| `match` (as macro) | — | Tier 3 (procedural) |


## Files

| File | Role |
|------|------|
| `src/syntax/expand/mod.rs` | Expander struct, `defmacro` handling, scope stamping |
| `src/syntax/expand/macro_expand.rs` | Substitution, quasiquote evaluation |
| `src/syntax/expand/quasiquote.rs` | Standalone quasiquote expansion |
| `src/syntax/expand/threading.rs` | `->` and `->>` |
| `src/syntax/expand/introspection.rs` | `macro?` and `expand-macro` |
| `src/syntax/expand/qualified.rs` | `module:name` resolution |
| `src/syntax/expand/tests.rs` | Expansion tests |
| `src/syntax/mod.rs` | `Syntax`, `SyntaxKind`, `ScopeId` |
| `src/hir/analyze/mod.rs` | `Analyzer`, `Scope`, `lookup()`, `bind()` |
| `src/pipeline.rs` | Compilation entry points |


## Open Questions

1. **Should Tier 1 gensym names be internable?** Currently `gensym`
   would produce strings like `__Gtmp0`. These go through normal symbol
   interning. An alternative is to use a special `SyntaxKind::Gensym(u32)`
   that can never collide with user symbols by construction. Simpler to
   start with strings; revisit if collision becomes a real problem.

2. **Should Tier 2 change the Scope representation?** The current
   `HashMap<String, BindingId>` is fast for string lookup. Scope-aware
   lookup needs to find all bindings with a given name and then pick the
   best match. A `HashMap<String, Vec<(Vec<ScopeId>, BindingId)>>` would
   work. Profile before optimizing.

3. **Should Tier 3 use `Syntax` or `Value` as its domain?** The plan
   says `Syntax` for hygiene preservation. But this means the evaluator
   can't reuse the existing VM. If we used `Value` with scope annotations
   threaded through, we could reuse more infrastructure. Needs prototyping.

4. **How do macros interact with the effect system?** A macro that
   expands to `(fiber/signal ...)` should produce code with `Yields`
   effect. Currently this works because effect inference happens after
   expansion. But procedural macros (Tier 3) might want to inspect or
   manipulate effects. Defer until Tier 3 is closer.

5. **Interaction between `set!` and scope-aware lookup.** `set!` goes
   through the Analyzer's `lookup()`. With scope-aware resolution, a
   macro that uses `set!` on a call-site variable must have the right
   scope set for the reference to resolve. This should work naturally
   (call-site arguments keep their original scopes) but needs careful
   testing, especially for mutable captures across closure boundaries.
