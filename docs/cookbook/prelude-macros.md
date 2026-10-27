# Adding a New Prelude Macro

<!-- audited: 2026-10-27 -->

A prelude macro is a `defmacro` that writes a new form in terms of forms Elle
already has.

### Files to modify

1. **[src/prelude.lisp](../../src/prelude.lisp)** — Add the `defmacro`
   definition.
2. **[src/hir/lint/tests/prelude.rs](../../src/hir/lint/tests/prelude.rs)** —
   Add a use of the macro to the list there. The test lints a use of every
   prelude macro and expects no finding.

No other Rust changes are needed.

### Step by step

**Step 1:** Add the macro to [src/prelude.lisp](../../src/prelude.lisp), under a
comment that shows the call and its expansion:

```lisp
## my-form - description of what it does
## (my-form arg body...) => expansion
(defmacro my-form (arg & body)
  `(let [tmp ,arg]
     (if tmp (begin ,;body) nil)))
```

**Step 2:** Add a use of `my-form` to the list in
[src/hir/lint/tests/prelude.rs](../../src/hir/lint/tests/prelude.rs), and run
`cargo test -p elle --lib hir::lint`. A warning there names a binding the
expansion makes. The user cannot rename or remove that binding, so correct the
macro. A `@` binding that no `assign` targets is the usual cause.

### How it works

1. [src/pipeline/sources.rs](../../src/pipeline/sources.rs) embeds the prelude
   in the binary as `PRELUDE`.
2. `Expander::load_prelude()` reads and expands the prelude under the primitive
   scope, which registers each `defmacro` in the Expander's macro table.
3. When user code contains `(my-form ...)`, the Expander recognizes it as a
   macro call, runs the macro body in the VM, and splices the result back as
   Syntax.
4. The HIR analyzer then analyzes the expanded code as it analyzes any other.

[macros](../macros.md) holds the macro syntax and hygiene, and
[stdlib](../stdlib.md) lists the prelude's macros.

### When to use a macro vs. a special form

Use a **prelude macro** when:
- The form can be expressed as a transformation of existing forms.
- No compile-time validation beyond arity is needed.
- No special lowering strategy is required.

Use a [special form](special-forms.md) when:
- Compile-time validation is needed (for example, `break` checking block scope).
- The form requires custom LIR emission (for example, `emit` splitting blocks).
- The form introduces new binding semantics (for example, `let`, `fn`).

---

## See also

- [Cookbook index](index.md)
