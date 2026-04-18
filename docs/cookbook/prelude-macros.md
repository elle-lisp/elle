# Adding a New Prelude Macro


Prelude macros are defined in `prelude.lisp` and loaded by the Expander
before user code. They are the preferred way to add new syntactic forms
that can be expressed in terms of existing forms.

### Files to modify

1. **`prelude.lisp`** — Add the `defmacro` definition.

That's it. No Rust changes needed.

### Step by step

**Step 1:** Add the macro to `prelude.lisp` (at the project root):

```lisp
## my-form - description of what it does
## (my-form arg body...) => expansion
(defmacro my-form (arg & body)
   `(let [tmp ,arg]
      (if tmp (begin ,;body) nil)))
```

### How it works

1. `prelude.lisp` is embedded into the binary via
    `include_str!("../../../prelude.lisp")` in
    `src/syntax/expand/mod.rs`.

2. `Expander::load_prelude()` parses and expands the prelude, which
    registers each `defmacro` in the Expander's macro table.

3. When user code contains `(my-form ...)`, the Expander recognizes it
    as a macro call, evaluates the macro body in the VM, and splices the
    result back as Syntax.

4. The expanded code is then analyzed normally by the HIR analyzer.

### Macro syntax reference

```lisp
(defmacro name (param1 param2 & rest-params)
   template)
```

- **Quasiquote** `` ` ``: template that allows unquoting.
- **Unquote** `,expr`: evaluate and insert a single value.
- **Unquote-splicing** `,;expr`: evaluate and splice a list/array.
- **`& rest`**: variadic parameter (collects remaining args as a list).
- **`gensym`**: generate a unique symbol (for hygienic temporaries).

### Existing prelude macros

| Macro | Expansion |
|-------|-----------|
| `defn` | `(def name (fn params body...))` |
| `let*` | Nested `let` (one binding per level) |
| `->` | Thread-first |
| `->>` | Thread-last |
| `when` | `(if test (begin body...) nil)` |
| `unless` | `(if test nil (begin body...))` |
| `try`/`catch` | Fiber-based error handling |
| `protect` | Returns `[success? value] array` |
| `defer` | Cleanup after body |
| `with` | Resource management (acquire/release) |
| `yield*` | Delegate to sub-generator |

### When to use a macro vs. a special form

Use a **prelude macro** when:
- The form can be expressed as a transformation of existing forms.
- No compile-time validation beyond arity is needed.
- No special lowering strategy is required.

Use a **special form** (Recipe 4) when:
- Compile-time validation is needed (e.g., `break` checking block scope).
- The form requires custom LIR emission (e.g., `yield` splitting blocks).
- The form introduces new binding semantics (e.g., `let`, `fn`).

---

## See also

- [Cookbook index](index.md)
