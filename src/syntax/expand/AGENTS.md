# syntax/expand

Hygienic macro expansion: macro definition, macro calls, quasiquote, and introspection.

## Responsibility

- Expand macros with hygiene (scope sets prevent accidental capture)
- Handle `defmacro` definitions
- Desugar `let*` to nested `let`
- Desugar `defn` to `(def name (fn ...))`
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
| `MacroDef` | Macro definition (name, params, rest_param, template, definition_scope, cached_transformer) |

## Data flow

```
Syntax (from reader)
    │
    ▼
Expander
    ├─► load prelude macros (when, unless, try, protect, defer, with, etc.)
    ├─► desugar defn to (def name (fn params body...))
    ├─► desugar let* to nested let (one binding at a time)
    ├─► check for macro calls
    ├─► compile & eval macro body in VM via pipeline::eval_syntax()
    ├─► convert result Value back to Syntax via from_value()
    ├─► add expansion scope (fresh ScopeId)
    ├─► handle macro? (check registry, return true/false literal)
    ├─► handle expand-macro (expand quoted form, wrap in quote)
    └─► recurse on result (with depth limit of 200)
    │
    ▼
Syntax (expanded)
```

## Hygiene via scope sets

Each macro expansion creates a fresh `ScopeId`. Identifiers introduced by the macro carry this scope. Identifiers from the call site don't. The Analyzer uses scope-set subset matching to prevent accidental capture:

```janet
(defmacro swap (a b)
  `(let ((tmp ,a)) (set ,a ,b) (set ,b tmp)))

(let ((tmp 10) (x 1) (y 2))
  (swap x y)
  tmp)  ; Still 10, not affected by macro's tmp
```

The macro's `tmp` has the expansion scope. The outer `tmp` has the call-site scope. They don't match, so no capture.

## Macro argument wrapping

Arguments are wrapped for binding in the let-expression that evaluates the macro body:

- **Atoms** (nil, bool, int, float, string, keyword) are wrapped via `Quote` to preserve runtime semantics (e.g., `false` stays falsy)
- **Symbols and compounds** are wrapped via `SyntaxLiteral(Value::syntax(arg))` to preserve scope sets

This hybrid approach ensures that:
1. Atoms evaluate correctly (e.g., `false` is falsy, not a symbol)
2. Scope sets survive the Value round-trip during macro expansion
3. `from_value()` unwraps syntax objects back to `Syntax`, preserving scopes

## Quasiquote expansion

Quasiquote is expanded to runtime list construction:

- `'x` → `(quote x)` (not expanded)
- `` `x `` → `(quote x)` (literal)
- `` `,x `` → `x` (unquote — evaluate)
- `` `,@x `` → `(splice x)` (unquote-splice — spread array/list)
- Nested quasiquotes increase depth; nested unquotes decrease depth

The `quasiquote_to_code()` function recursively converts quasiquote forms to `list`, `cons`, `quote`, and `splice` calls that construct the result at runtime.

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

## Expander struct

The `Expander` maintains:

- `macros: HashMap<String, MacroDef>` — Registered macro definitions
- `compile_time_env: HashMap<String, Value>` — Values defined in `begin-for-syntax` blocks. Always starts empty (the custom `Clone` impl resets it). Visible to macro bodies compiled via `eval_syntax` through `Analyzer::bind_compile_time_env`.
- `next_scope_id: u32` — Counter for generating fresh scope IDs
- `expansion_depth: usize` — Current recursion depth (bounded at 200)

## Prelude macros

The standard prelude (`prelude.lisp`) defines:
- `defn` — shorthand for `(def name (fn ...))`
- `let*` — sequential let bindings (desugared to nested `let`)
- `->` — thread-first macro
- `->>` — thread-last macro
- `when` — conditional without else
- `unless` — conditional with negated test
- `try`/`catch` — error signal handling (sugar over fiber primitives)
- `protect` — finally block
- `defer` — cleanup on exit
- `with` — resource management
- `yield*` — yield multiple values
- `case` — pattern matching (legacy)
- `if-let` — conditional binding
- `when-let` — conditional binding without else
- `forever` — infinite loop

These are loaded by `Expander::load_prelude()` before user code expansion.

## Syntax objects in the Value system

`SyntaxKind::SyntaxLiteral(Value)` is an internal-only variant used by `expand_macro_call_inner` to inject `Value::syntax(arg)` into the compilation pipeline. This preserves scope sets through the Value round-trip during macro expansion. The Analyzer handles it by producing `HirKind::Quote(value)`.

## Hygiene escape hatch: `datum->syntax`

`(datum->syntax context datum)` creates a syntax object with the context's scope set and `scope_exempt: true`. This prevents `add_scope_recursive` from adding the intro scope, so the datum resolves at the call site. Used for anaphoric macros:

```janet
(defmacro aif (test then else)
  `(let ((,(datum->syntax test 'it) ,test))
     (if ,(datum->syntax test 'it) ,then ,else)))
```

`(syntax->datum stx)` strips scope information, returning the plain value.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~400 | `Expander` struct, macro registry, entry point, desugaring |
| `macro_expand.rs` | ~170 | VM-based macro expansion via `eval_syntax` |
| `quasiquote.rs` | ~160 | Quasiquote-to-code conversion |
| `introspection.rs` | ~100 | `macro?` and `expand-macro` |
| `compiletime.rs` | ~80 | `begin-for-syntax` handler |
| `tests.rs` | ~540 | Expansion tests |

## Invariants

1. **Scopes are additive, with one exception.** `add_scope()` never removes. `add_scope_recursive()` skips nodes with `scope_exempt: true` (set by `datum->syntax` to prevent intro scope stamping on nodes that should resolve at the call site). `scope_exempt` only affects `add_scope_recursive`, not `add_scope`. Two identifiers match only if their scope sets are compatible.

2. **Quote forms are not expanded.** `'x` remains `Quote(Symbol("x"))`. The analyzer handles quote specially.

3. **Quasiquote/unquote must be expanded.** If analysis sees raw `Quasiquote`, `Unquote`, or `UnquoteSplicing`, expansion failed.

4. **Macro arity is checked.** Wrong argument count → error, not silent misbehavior.

5. **macro? and expand-macro are compile-time.** Both are handled by the Expander during expansion, not as runtime primitives. `macro?` checks the macro registry and returns a literal `true` or `false`. `expand-macro` expands a quoted form and wraps the result in quote.

6. **Macro bodies are VM-evaluated.** Macro arguments are quoted and passed to the macro body, which is compiled and executed in the real VM via `pipeline::eval_syntax()`. The result Value is converted back to Syntax via `from_value()`. Macros must use quasiquote to return code templates.

7. **Cached transformer is populated on first use, per pipeline call.**
    `MacroDef.cached_transformer` holds the compiled `(fn (params...) template)`
    closure after first expansion. Cloning `MacroDef` copies the `Value` (cheap;
    it's `Copy` and the closure's heap data is `Rc`). The original in the
    `CompilationCache` does NOT see the update (different `RefCell`) — the
    cache warms per pipeline call, not globally. This is by design.

8. **Qualified symbols pass through expansion unchanged.** `module:name` is recognized by the lexer as a single token. The Expander does not transform it. The Analyzer desugars it to nested `get` calls.

9. **Expansion depth is bounded.** Max 200 levels to prevent infinite expansion. If exceeded, compilation fails with "macro expansion depth exceeded" error.

10. **`compile_time_env` is always reset to empty on clone.** This prevents compile-time defs from leaking between pipeline calls via the cached Expander. See the manual `Clone` impl in `mod.rs`.

## When to modify

- **Adding a new prelude macro**: Add to `prelude.lisp` (project root), not here
- **Changing macro expansion algorithm**: Update `mod.rs::expand()`
- **Changing quasiquote semantics**: Update `quasiquote.rs`
- **Changing macro argument wrapping**: Update `macro_expand.rs::wrap_macro_arg()`
- **Adding introspection forms**: Update `introspection.rs`

## Common pitfalls

- **Breaking hygiene**: When creating synthetic identifiers, ensure they carry the correct scope set
- **Forgetting to expand recursively**: After macro expansion, the result must be recursively expanded (with depth limit)
- **Not preserving scope sets**: Arguments wrapped as `SyntaxLiteral` must preserve their scope sets through the Value round-trip
- **Conflating `Quote` and `SyntaxLiteral`**: `Quote` is for atoms; `SyntaxLiteral` is for compounds that need scope preservation
- **Not handling improper lists**: Macros cannot return improper lists (e.g., `(cons 1 2)`). The `from_value()` conversion requires proper lists.
