# syntax/expand

<!-- audited: 2026-10-07 -->

Hygienic macro expansion: macro definition, macro calls, quasiquote, and introspection.

## Responsibility

- Expand macros with hygiene (scope sets prevent accidental capture)
- Handle `defmacro` definitions
- Expand `syntax-case` into pattern-matching code
- Expand quasiquote to runtime list construction
- Provide `macro?` and `expand-macro` introspection
- Handle `begin-for-syntax` compile-time definitions
- Load the standard prelude macros

Does NOT:
- Parse source (that's `reader`)
- Resolve bindings (that's `hir/analyze`)
- Generate code (that's `lir`)
- Execute code (that's `vm`)

## Key types

| Type | Purpose |
|------|---------|
| `Expander` | Main struct that expands macros |
| `MacroDef` | Macro definition: name, parameters, template, transformer cell ([macrodef.rs](macrodef.rs)) |
| `MacroParams` | The required, `&opt` and `&` rest parameter names, each set by a builder step of its own |
| `TransformerCell` | The compiled transformer of one definition, shared by every clone of it |

## Data flow

```
Syntax (from reader)
    │
    ▼
Expander
    ├─► load prelude macros (when, unless, try, protect, defer, with, etc.)
    ├─► check for macro calls
    ├─► compile the transformer once per definition via pipeline::eval_syntax()
    ├─► call the transformer on the arguments via VM::call_closure()
    ├─► convert result Value back to Syntax via from_value()
    ├─► flip the expansion's intro scope on the result
    ├─► handle macro? (check registry, return true/false literal)
    ├─► handle expand-macro (expand quoted form, wrap in quote)
    └─► recurse on result (with depth limit of 200)
    │
    ▼
Syntax (expanded)
```

## Hygiene via scope sets

Each macro expansion creates a fresh `ScopeId`. Identifiers introduced by the macro carry this scope. Identifiers from the call site don't. The Analyzer uses scope-set subset matching to prevent accidental capture:

```lisp
(defmacro swap (a b)
  `(let [tmp ,a] (assign ,a ,b) (assign ,b tmp)))

(let [@tmp 10 @x 1 @y 2]
  (swap x y)
  tmp)  # still 10, not affected by the macro's tmp
```

The macro's `tmp` has the expansion scope. The outer `tmp` has the call-site scope. They don't match, so no capture.

## Macro argument wrapping

A call passes each argument to the transformer closure as a `Value` (`wrap_macro_arg_value`):

- **Atoms** (nil, bool, int, float, string, keyword) pass as the equivalent `Value`, so `false` stays falsy.
- **Symbols and compound forms** pass as syntax objects (`Value::syntax`), so their scope sets survive the call.

`from_value()` unwraps the syntax objects in the result back to `Syntax`, with their scopes.

## Quasiquote expansion

Quasiquote is expanded to code that constructs the result at runtime:

- `'x` → `(quote x)` (not expanded)
- `` `sym `` → a `SyntaxLiteral` node, which carries the template symbol with its scopes
- `` `42 `` → `(quote 42)`: every other form, an atom or a struct for example, is quoted
- `` `,x `` → `x` (unquote — evaluate)
- `` `(a ,;xs) `` → `(append (list a) xs)`: a splice cuts the list into segments that `append` joins
- `` `[a ,b] `` → `(array a b)`
- Nested quasiquotes increase depth; nested unquotes decrease depth

`quasiquote_to_code()` does the conversion, in [quasiquote.rs](quasiquote.rs).

## Introspection

Two compile-time introspection forms:

- **`(macro? name)`** — Check if `name` is a registered macro. Returns a literal `true` or `false` (not evaluated at runtime).
- **`(expand-macro form)`** — Expand a quoted form using the current macro registry. Returns the expanded form wrapped in `quote`.

Both are handled by the Expander during expansion, not as runtime primitives.

## Expander dispatch

Special forms recognized before macro calls:

- **`defmacro` / `define-macro`** — Define a macro. Stored in the macro registry.
- **`macro?`** — Check if a name is a registered macro. Returns a literal boolean.
- **`expand-macro`** — Expand a quoted form. Returns the expanded form wrapped in quote.
- **`begin-for-syntax`** — Compile-time definitions. Evaluates `(def <symbol> <expr>)` forms via `eval_syntax` and stores the resulting values in `Expander.compile_time_env`. Returns nil. Processed in `src/syntax/expand/compiletime.rs`. Only plain-symbol `def` forms are supported; all others are rejected at expansion time.
- **`syntax-case`** — Pattern matching on syntax objects. Recognized before macro calls. Generates a chain of `let`/`if` forms using the syntax predicates. The scrutinee is bound to a gensym at the outermost level. No `eval_syntax` calls — pure code generation. Implemented in `src/syntax/expand/syntaxcase.rs`.

## Expander struct

The `Expander` maintains:

- `macros: HashMap<String, MacroDef>` — Registered macro definitions
- `compile_time_env: HashMap<String, Value>` — Values defined in `begin-for-syntax` blocks. Always starts empty (the custom `Clone` impl resets it). Visible to macro bodies compiled via `eval_syntax` through `Analyzer::bind_compile_time_env`.
- `core_env: HashMap<String, Value>` — core.lisp's exports, which macro bodies resolve. A clone keeps it.
- `eval_meta: Rc<PrimitiveMeta>` — The primitives and stdlib exports that macro bodies compile against.
- `next_scope_id: u32` — Counter for generating fresh scope IDs
- `expansion_depth: usize` — Current recursion depth (bounded at 200)

## Prelude macros

[src/prelude.lisp](../../prelude.lisp) defines the everyday forms as ordinary macros, and [docs/macros.md](../../../docs/macros.md) lists them. `Expander::load_prelude()` loads them before user code expands.

## Syntax objects in the Value system

`SyntaxKind::SyntaxLiteral(SynRef)` is an internal-only variant that carries a hygiene-bearing template symbol as plain compile-time data. Quasiquote creates it so a template symbol's scope set survives the Value round-trip during macro expansion; the Analyzer materializes it as an ordinary allocation per execution via `ConstTemplate::SyntaxSymbol`.

## Arenas

The expander holds two: `arena` is the working arena of the unit under expansion (the pipeline sets it per unit), and `templates` is the instance's process-root arena, where `handle_defmacro` copies a macro template so it outlives the unit that defined it. `map_scope_recursive` COPIES — subtrees are shared by pointer, so a walk that wrote through the source would stamp trees the caller still holds. See docs/impl/syntax.md § "Where a node lives".

## Hygiene escape hatch: `datum->syntax`

`(datum->syntax context datum)` creates a syntax object with the context's scope set and `scope_exempt: true`. Neither the intro-scope stamp on the arguments nor the flip on the result touches it, so the datum resolves at the call site. Used for anaphoric macros:

```lisp
(defmacro aif (test then else)
  `(let [,(datum->syntax test 'it) ,test]
     (if ,(datum->syntax test 'it) ,then ,else)))
```

`(syntax->datum stx)` strips scope information, returning the plain value.

## Invariants

1. **Scope walks skip exempt nodes.** `add_scope_recursive()` stamps a scope on a tree. `flip_scope_recursive()` adds the intro scope where a node lacks it and removes it where a node has it. Both skip a node with `scope_exempt: true`, which `datum->syntax` sets. Two identifiers match only if their scope sets are compatible.

2. **Quote forms are not expanded.** `'x` remains `Quote(Symbol("x"))`. The analyzer handles quote specially.

3. **Quasiquote/unquote must be expanded.** If analysis sees raw `Quasiquote`, `Unquote`, or `UnquoteSplicing`, expansion failed.

4. **Macro arity is checked.** Wrong argument count → error, not silent misbehavior.

5. **macro? and expand-macro are compile-time.** Both are handled by the Expander during expansion, not as runtime primitives. `macro?` checks the macro registry and returns a literal `true` or `false`. `expand-macro` expands a quoted form and wraps the result in quote.

6. **Macro bodies are VM-evaluated.** The first expansion of a definition compiles `(fn (params...) template)` via `pipeline::eval_syntax()`. Every expansion calls that closure on the VM, and `from_value()` converts the result back to Syntax.

7. **A transformer cell owns its transformer's region.**
    A definition's `TransformerCell` holds the compiled closure after the first
    expansion, with one reference to the region it lives in and the heap it
    was compiled on. Every clone of the definition shares the cell, so the
    first compile that expands the macro fills it and every later compile
    reuses it. The last clone to drop releases the reference on that heap.
    A definition that a redefinition replaces therefore frees its transformer
    when it drops, and so does one that only a per-compile clone or the VM's
    `eval` expander held. Teardown empties the instance's cells
    (`release_cached_transformers`) and drops the `eval` expanders before the
    sweep, so no transformer is left as residue.

    An expansion fills the cell of the definition it looked up. A `defmacro`
    of the same name that runs while the transformer compiles defines a new
    macro, with an empty cell of its own.

8. **Qualified symbols pass through expansion unchanged.** `module:name` is recognized by the lexer as a single token. The Expander does not transform it. The Analyzer desugars it to nested `get` calls.

9. **Expansion depth is bounded.** Max 200 levels to prevent infinite expansion. If exceeded, compilation fails with "macro expansion depth exceeded" error.

10. **`compile_time_env` is always reset to empty on clone.** This prevents compile-time defs from leaking between pipeline calls via the cached Expander. See the manual `Clone` impl in `mod.rs`.

11. **`syntax-case` is pure code generation, not expansion-time evaluation.** The scrutinee expression is not evaluated at expansion time (it may be a macro parameter with no value). Instead, `syntax-case` generates a chain of `let`/`if` forms that perform pattern matching at runtime using the syntax predicates. The generated code runs when the macro transformer closure executes inside the VM.

## When to modify

- **Adding a new prelude macro**: Add to `src/prelude.lisp`, not here
- **Changing macro expansion algorithm**: Update `mod.rs::expand()`
- **Changing quasiquote semantics**: Update `quasiquote.rs`
- **Changing macro argument wrapping**: Update `macro_expand.rs::wrap_macro_arg_value()`
- **Adding introspection forms**: Update `introspection.rs`

## Common pitfalls

- **Breaking hygiene**: When creating synthetic identifiers, ensure they carry the correct scope set
- **Forgetting to expand recursively**: After macro expansion, the result must be recursively expanded (with depth limit)
- **Not preserving scope sets**: A symbol or compound argument crosses the transformer call as a syntax object, which must keep its scope set through the Value round-trip
- **Conflating `quote` and `SyntaxLiteral`**: a template atom is quoted; a template symbol is a `SyntaxLiteral`, which keeps its scopes
- **Not handling improper lists**: Macros cannot return improper lists (for example, `(pair 1 2)`). The `from_value()` conversion requires proper lists.
