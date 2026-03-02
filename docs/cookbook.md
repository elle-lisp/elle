# Cookbook: Cross-Cutting Recipes

Step-by-step recipes for the most common additions to the Elle codebase.
Each recipe lists the exact files to modify, in order, with the types and
functions involved.

---

## 1. Adding a New Primitive Function

A primitive is a Rust function callable from Elle. All primitives have the
same signature: `fn(&[Value]) -> (SignalBits, Value)`.

### Files to modify

1. **`src/primitives/<module>.rs`** — Implement the function and add it to
   the module's `PRIMITIVES` table.

2. **`src/primitives/registration.rs`** — Only if creating a *new module
   file*. Add the module's `PRIMITIVES` to the `ALL_TABLES` array.

3. **`src/primitives/mod.rs`** — Only if creating a *new module file*. Add
   `pub mod <module>;`.

### Step by step

**Step 1: Write the function** in the appropriate module (e.g.,
`src/primitives/string.rs` for string operations).

```rust
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

pub fn prim_my_func(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error",
                format!("my-func: expected 1 argument, got {}", args.len())),
        );
    }
    // ... implementation ...
    (SIG_OK, Value::int(42))
}
```

**Step 2: Add to the module's `PRIMITIVES` table** (a `const &[PrimitiveDef]`
at the bottom of the file):

```rust
use crate::primitives::def::PrimitiveDef;
use crate::effects::Effect;
use crate::value::types::Arity;

pub const PRIMITIVES: &[PrimitiveDef] = &[
    // ... existing entries ...
    PrimitiveDef {
        name: "my-func",
        func: prim_my_func,
        effect: Effect::none(),       // or Effect::yields() if it signals
        arity: Arity::Exact(1),
        doc: "One-line description.",
        params: &["x"],
        category: "my-category",
        example: "(my-func 42) #=> 42",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    },
];
```

**Step 3 (new module only):** Add the module to `ALL_TABLES` in
`src/primitives/registration.rs`:

```rust
pub(crate) const ALL_TABLES: &[&[PrimitiveDef]] = &[
    // ... existing entries ...
    my_module::PRIMITIVES,
];
```

And declare it in `src/primitives/mod.rs`:

```rust
pub mod my_module;
```

### How it works

`register_primitives()` in `registration.rs` iterates `ALL_TABLES`. For
each `PrimitiveDef`, it:
- Interns the name via `symbols.intern(def.name)` → `SymbolId`
- Stores `Value::native_fn(def.func)` in `vm.globals[sym_id]`
- Records effect and arity in `PrimitiveMeta`
- Registers aliases identically

At runtime, `LoadGlobal` fetches the `NativeFn` value, and `Call`
dispatches it via `handle_primitive_signal()` in `src/vm/signal.rs`.

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `NativeFn` | `src/value/types.rs` | `fn(&[Value]) -> (SignalBits, Value)` |
| `PrimitiveDef` | `src/primitives/def.rs` | Declarative metadata struct |
| `PrimitiveMeta` | `src/primitives/def.rs` | Collected effects/arities maps |
| `Arity` | `src/value/types.rs` | `Exact(n)`, `AtLeast(n)`, `Range(min, max)` |
| `Effect` | `src/effects/` | `Effect::none()`, `Effect::yields()` |

### Conventions

- Return `(SIG_OK, value)` for success.
- Return `(SIG_ERROR, error_val("kind", "message"))` for errors.
- Validate arity and types at the top of the function.
- No primitive has VM access. If you need VM interaction, return
  `(SIG_RESUME, fiber_value)` or `(SIG_QUERY, cons(keyword, arg))`.

---

## 2. Adding a New Heap Type

A heap type is a new kind of runtime value stored behind a NaN-boxed
pointer. Use the `Buffer` type as a reference — it's a recent, clean
example.

### Files to modify (in order)

1. **`src/value/heap.rs`** — Add variant to `HeapObject` enum, add tag to
   `HeapTag` enum, add arms to `tag()`, `type_name()`, and `Debug`.

2. **`src/value/repr/constructors.rs`** — Add `Value::my_type(...)` constructor.

3. **`src/value/repr/accessors.rs`** — Add `is_my_type()` predicate and
   `as_my_type()` accessor.

4. **`src/value/display.rs`** — Add `Display` and `Debug` formatting arms.

5. **`src/value/repr/traits.rs`** — Add `PartialEq` arm for the new type.

6. **`src/value/send.rs`** — Add `SendValue` variant (if sendable) or
   rejection arm (if not).

7. **`src/primitives/json/serializer.rs`** — Add arm to `serialize_value`
   (exhaustive `HeapTag` match).

8. **`src/formatter/core.rs`** — Add arm to `format_value` (exhaustive
   `HeapObject` match).

9. **`src/syntax/convert.rs`** — Update `Syntax::from_value()` if the type
   can appear in macro results (Value → Syntax conversion).

### Step by step

**Step 1: `src/value/heap.rs`** — Three changes:

```rust
// In HeapTag enum — assign next available discriminant:
pub enum HeapTag {
    // ... existing ...
    MyType = 22,  // next after Buffer = 21
}

// In HeapObject enum:
pub enum HeapObject {
    // ... existing ...
    /// Description of my type
    MyType(MyTypeData),
}

// In HeapObject::tag():
HeapObject::MyType(_) => HeapTag::MyType,

// In HeapObject::type_name():
HeapObject::MyType(_) => "my-type",

// In HeapObject Debug impl:
HeapObject::MyType(_) => write!(f, "<my-type>"),
```

**Step 2: `src/value/repr/constructors.rs`** — Add constructor:

```rust
impl Value {
    pub fn my_type(data: MyTypeData) -> Self {
        use crate::value::heap::{alloc, HeapObject};
        alloc(HeapObject::MyType(data))
    }
}
```

**Step 3: `src/value/repr/accessors.rs`** — Add predicate and accessor:

```rust
impl Value {
    pub fn is_my_type(&self) -> bool {
        use crate::value::heap::HeapTag;
        self.heap_tag() == Some(HeapTag::MyType)
    }

    pub fn as_my_type(&self) -> Option<&MyTypeData> {
        use crate::value::heap::{deref, HeapObject};
        if !self.is_heap() { return None; }
        match unsafe { deref(*self) } {
            HeapObject::MyType(data) => Some(data),
            _ => None,
        }
    }
}
```

**Step 4: `src/value/display.rs`** — Add arms in both `Display` and
`Debug` impls (search for existing heap type formatting like `Buffer`):

```rust
// In Display impl, after buffer handling:
if let Some(data) = self.as_my_type() {
    return write!(f, "<my-type:{}>", data);
}

// Debug impl delegates to Display for most heap types.
```

**Step 5: `src/value/repr/traits.rs`** — Add `PartialEq` arm:

```rust
// In the (self_obj, other_obj) match:
(HeapObject::MyType(a), HeapObject::MyType(b)) => a == b,
// or for reference equality:
(HeapObject::MyType(_), HeapObject::MyType(_)) => {
    std::ptr::eq(self_obj as *const _, other_obj as *const _)
}
```

**Step 6: `src/value/send.rs`** — Add to `SendValue`:

```rust
// If sendable, add a variant and implement from_value/into_value:
pub enum SendValue {
    // ...
    MyType(MyTypeOwnedData),
}

// In from_value():
HeapObject::MyType(data) => Ok(SendValue::MyType(data.clone())),

// In into_value():
SendValue::MyType(data) => alloc(HeapObject::MyType(data)),

// If NOT sendable:
HeapObject::MyType(_) => Err("Cannot send my-type".to_string()),
```

### After adding the type

You'll likely want primitives to create and manipulate it (see Recipe 1)
and a type-check predicate in `src/primitives/types.rs`.

---

## 3. Adding a New Bytecode Instruction

An instruction flows through three layers: definition (`compiler`),
emission (`lir`), and execution (`vm`).

### Files to modify (in order)

1. **`src/compiler/bytecode.rs`** — Add variant to `Instruction` enum.

2. **`src/compiler/bytecode_debug.rs`** — Add disassembly formatting.

3. **`src/lir/types.rs`** — Add variant to `LirInstr` enum.

4. **`src/lir/emit.rs`** — Add emission case in `emit_instr()`.

5. **`src/vm/dispatch.rs`** — Add dispatch arm in the main loop.

6. **`src/vm/<handler>.rs`** — Implement the handler function.

### Step by step

**Step 1: `src/compiler/bytecode.rs`** — Add to `Instruction` enum.
**Add at the end** (byte values are positional via `#[repr(u8)]`):

```rust
#[repr(u8)]
pub enum Instruction {
    // ... existing variants ...
    /// Description of new instruction
    MyInstr,
}
```

**Step 2: `src/compiler/bytecode_debug.rs`** — Add disassembly in
`disassemble_lines()`. If the instruction has operands, add a match arm;
otherwise the `_ => {}` catch-all handles it:

```rust
Instruction::MyInstr => {
    // If it has a u16 operand:
    if i + 1 < instructions.len() {
        let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
        line.push_str(&format!(" (index={})", idx));
        i += 2;
    }
}
```

**Step 3: `src/lir/types.rs`** — Add to `LirInstr` enum:

```rust
pub enum LirInstr {
    // ... existing ...
    /// Description
    MyInstr { dst: Reg, src: Reg },
}
```

**Step 4: `src/lir/emit.rs`** — Add emission in `emit_instr()`:

```rust
LirInstr::MyInstr { dst, src } => {
    self.ensure_on_top(*src);
    self.bytecode.emit(Instruction::MyInstr);
    self.pop();          // consumed input
    self.push_reg(*dst); // produced output
}
```

The emitter uses stack simulation. Key helpers:
- `ensure_on_top(reg)` — ensures a register's value is at stack top
- `ensure_binary_on_top(lhs, rhs)` — ensures two regs are top-2
- `push_reg(reg)` / `pop()` — track simulated stack

**Step 5: `src/vm/dispatch.rs`** — Add dispatch arm in
`execute_bytecode_inner_impl()`:

```rust
Instruction::MyInstr => {
    my_handler::handle_my_instr(self);
}
```

**Step 6: `src/vm/<handler>.rs`** — Implement the handler. Follow the
pattern in `src/vm/data.rs` or `src/vm/types.rs`:

```rust
pub fn handle_my_instr(vm: &mut VM) {
    let value = vm.fiber.stack.pop()
        .expect("VM bug: stack underflow on MyInstr");
    // ... transform value ...
    vm.fiber.stack.push(result);
}
```

### Conventions

- Stack underflow is a VM bug → `panic!` (not a user error).
- User errors → `vm.fiber.signal = Some((SIG_ERROR, error_val(...)))`,
  push `Value::NIL`, return normally.
- Instructions that consume N values and produce M values must match the
  emitter's stack simulation exactly.

---

## 4. Adding a New Special Form

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
    let effect = arg.effect.combine(body.effect);
    Ok(Hir::new(
        HirKind::MyForm {
            arg: Box::new(arg),
            body: Box::new(body),
        },
        span,
        effect,
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

## 5. Adding a New Lint Rule

Linting operates on HIR trees. Rules live in `src/lint/rules.rs`; the
tree walker lives in `src/hir/lint.rs`.

### Files to modify (in order)

1. **`src/lint/rules.rs`** — Implement the rule function.

2. **`src/hir/lint.rs`** — Call the rule from the appropriate `HirKind`
   arm in `HirLinter::check()`.

3. **`src/lint/mod.rs`** — Re-export if the rule is public.

### Step by step

**Step 1: `src/lint/rules.rs`** — Write the rule. Rules take context and
push `Diagnostic`s:

```rust
use super::diagnostics::{Diagnostic, Severity};
use crate::reader::SourceLoc;

pub fn check_my_rule(
    context_data: &str,
    location: &Option<SourceLoc>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if /* violation detected */ {
        diagnostics.push(Diagnostic::new(
            Severity::Warning,
            "W004",                    // unique code
            "my-rule-name",            // kebab-case rule name
            "description of the issue",
            location.clone(),
        ).with_suggestions(vec![
            "how to fix it".to_string(),
        ]));
    }
}
```

**Step 2: `src/hir/lint.rs`** — Call the rule from the tree walker. Find
the appropriate `HirKind` match arm in `check()`:

```rust
// Example: check all let bindings for some property
HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
    for (binding, init) in bindings {
        // Call your rule here:
        if let Some(sym_name) = symbols.name(binding.name()) {
            rules::check_my_rule(sym_name, &loc, &mut self.diagnostics);
        }
        self.check(init, symbols);
    }
    self.check(body, symbols);
}
```

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Diagnostic` | `src/lint/diagnostics.rs` | Finding with severity, code, message, location |
| `Severity` | `src/lint/diagnostics.rs` | `Info`, `Warning`, `Error` |
| `HirLinter` | `src/hir/lint.rs` | Tree walker that calls rules |
| `Linter` | `src/lint/cli.rs` | CLI wrapper that runs `HirLinter` |

### Diagnostic codes

- `W001` — naming-kebab-case
- `W002` — arity-mismatch
- `W003` — non-exhaustive-match
- Use `W004+` for new warnings, `E00x` for errors, `I00x` for info.

### How linting runs

1. `Linter::lint_str()` (in `src/lint/cli.rs`) calls `analyze_all()` to
   get HIR.
2. For each analysis result, it creates a `HirLinter` and calls
   `hir_linter.lint(&analysis.hir, &symbols)`.
3. `HirLinter::check()` recursively walks the HIR tree, calling rule
   functions that push `Diagnostic`s.
4. The LSP (`src/lsp/state.rs`) uses the same `HirLinter` for real-time
   diagnostics.

---

## 6. Adding a New Prelude Macro

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
  `(let ((tmp ,arg))
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
| `protect` | Returns `[success? value]` tuple |
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
