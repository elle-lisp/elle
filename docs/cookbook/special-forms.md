# Adding a New Special Form


A special form is a syntactic construct recognized by the analyzer (not a
function call). It flows: Syntax → HIR → LIR.

### Files to modify (in order)

1. **`src/hir/expr.rs`** — Add variant to `HirKind` enum.

2. **`src/hir/analyze/forms.rs`** — Add name to the dispatch table in
   `analyze_expr()` and implement `analyze_my_form()`.

3. **`src/lir/lower/expr.rs`** — Add dispatch arm in `lower_expr()`.

4. **`src/lir/lower/<file>.rs`** — Implement `lower_my_form()`.

5. **`src/hir/lint.rs`** — Add traversal arm in `HirLinter::check()`.

6. **`src/hir/tailcall.rs`** — Add traversal arm for tail-call marking.

7. **`src/hir/symbols.rs`** — Add traversal arm for symbol extraction.

### Step by step

**Step 1: `src/hir/expr.rs`** — Add to `HirKind`:

```rust
pub enum HirKind {
    // ... existing ...
    /// Description of the form
    MyForm {
        arg: Box<Hir>,
        body: Box<Hir>,
    },
}
```

**Step 2: `src/hir/analyze/forms.rs`** — Register in the dispatch table
(the `match name.as_str()` block inside `SyntaxKind::List`):

```rust
// In analyze_expr(), inside the special form dispatch:
"my-form" => return self.analyze_my_form(items, span),
```

Then implement the analysis function (in `forms.rs` or a separate file
under `src/hir/analyze/`):

```rust
pub(crate) fn analyze_my_form(
    &mut self,
    items: &[Syntax],
    span: Span,
) -> Result<Hir, String> {
    if items.len() != 3 {
        return Err(format!("{}: my-form requires 2 arguments", span));
    }
    let arg = self.analyze_expr(&items[1])?;
    let body = self.analyze_expr(&items[2])?;
    let signal = arg.signal.combine(body.signal);
    Ok(Hir::new(
        HirKind::MyForm {
            arg: Box::new(arg),
            body: Box::new(body),
        },
        span,
        signal,
    ))
}
```

**Step 3: `src/lir/lower/expr.rs`** — Add dispatch:

```rust
HirKind::MyForm { arg, body } => self.lower_my_form(arg, body),
```

**Step 4: `src/lir/lower/control.rs`** (or appropriate file) — Implement
lowering. This is where you decide what LIR instructions to emit:

```rust
pub(super) fn lower_my_form(
    &mut self,
    arg: &Hir,
    body: &Hir,
) -> Result<Reg, String> {
    let arg_reg = self.lower_expr(arg)?;
    let body_reg = self.lower_expr(body)?;
    // ... emit LIR instructions ...
    Ok(body_reg)
}
```

**Step 5: `src/hir/lint.rs`** — Add traversal in `check()`:

```rust
HirKind::MyForm { arg, body } => {
    self.check(arg, symbols);
    self.check(body, symbols);
}
```

**Step 6: `src/hir/tailcall.rs`** — Add traversal for tail-call analysis.

**Step 7: `src/hir/symbols.rs`** — Add traversal for IDE symbol extraction.

### Key insight

Most "special forms" in Elle are actually prelude macros (see Recipe 6).
Only add a true special form if:
- It needs compile-time validation (like `break` checking block boundaries)
- It requires special lowering (like `yield` splitting basic blocks)
- It cannot be expressed as a macro over existing forms

---

## See also

- [Cookbook index](index.md)
