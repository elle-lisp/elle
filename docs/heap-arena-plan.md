# Heap Arena Plan

## Problem

`Value` is `Copy` (a `u64`). Heap objects are allocated via `Rc::into_raw` in
`heap::alloc()`. Since `Value` has no `Drop`, the `Rc` refcount is never
decremented. Every heap object leaks for the lifetime of the thread.

This is fine for single-shot execution (process exits, OS reclaims memory).
It breaks when the test suite creates ~2,560 VMs in one process: macro
expansion during `define_graph_functions` allocates ~10.9 MB of temporary
heap objects per call, none ever freed. The full suite OOM-kills.

## Root cause confirmed by tests

| Test | Finding |
|------|---------|
| h5k  | `define_graph_functions` leaks 10,920 KB/iter; other stdlib defs ~100-200 KB |
| h5l  | Leak scales super-linearly with `each` macro nesting depth |
| h6   | Compilation cache VM globals are stable (320, constant) |
| h7   | String interner stable at 21 entries after first iter |

The leak is in heap allocations created during macro expansion: cons cells
(quasiquote templates), syntax objects, bindings (HIR analysis), closures
(intermediate bytecode for macro bodies). All allocated via `heap::alloc()`,
none ever freed.

## Design: thread-local heap arena with mark/release

### Core idea

Replace `Rc::into_raw` in `alloc()` with pushing a `Box<HeapObject>` into a
thread-local arena Vec. Add `mark()` / `release(mark)` operations. Mark saves
the current length; release truncates back, dropping the freed objects.

Mark/release nests like a stack — inner marks are released before outer marks.

### Arena structure

```rust
thread_local! {
    static HEAP_ARENA: RefCell<HeapArena> = RefCell::new(HeapArena::new());
}

struct HeapArena {
    /// All live heap allocations. Box provides a stable pointer.
    objects: Vec<Box<HeapObject>>,
}

/// Opaque mark for arena scope management.
pub struct ArenaMark(usize);

/// RAII guard that releases the arena to a saved mark on drop.
/// Ensures release happens even on error paths (? operator, panics).
pub struct ArenaGuard(Option<ArenaMark>);

impl ArenaGuard {
    pub fn new() -> Self {
        ArenaGuard(Some(heap_arena_mark()))
    }
}

impl Drop for ArenaGuard {
    fn drop(&mut self) {
        if let Some(mark) = self.0.take() {
            heap_arena_release(mark);
        }
    }
}
```

### Allocation

```rust
pub fn alloc(obj: HeapObject) -> Value {
    HEAP_ARENA.with(|arena| {
        let mut a = arena.borrow_mut();
        let boxed = Box::new(obj);
        let ptr = &*boxed as *const HeapObject as *const ();
        a.objects.push(boxed);
        Value::from_heap_ptr(ptr)
    })
}
```

`Box<HeapObject>` gives a stable heap pointer. The Box is stored in the Vec.
When the Vec grows and reallocates, the Boxes (heap pointers) remain stable —
only the Vec's internal pointer-array moves, not the pointed-to objects.

### Permanent allocations

Some heap objects must outlive any mark/release scope:

- **NativeFn**: function pointers registered as VM globals. Created once per
  `register_primitives`. Must survive all arena resets.

Add `alloc_permanent(obj: HeapObject) -> Value` that uses the current
`Rc::into_raw` path — intentionally leaks, bypasses arena tracking. Change
`Value::native_fn()` to use `alloc_permanent`.

- **Interned strings**: `intern_string()` in `value/intern.rs` uses its own
  `Rc::new` and stores in the `STRING_INTERNER` HashMap. Does NOT call
  `heap::alloc()`. No change needed.

- **Keywords**: encoded as `TAG_PTRVAL` pointing to interned strings. Not heap
  pointers. No change needed.

### Mark / Release

```rust
pub fn heap_arena_mark() -> ArenaMark {
    HEAP_ARENA.with(|arena| ArenaMark(arena.borrow().objects.len()))
}

pub fn heap_arena_release(mark: ArenaMark) {
    HEAP_ARENA.with(|arena| arena.borrow_mut().objects.truncate(mark.0))
    // truncate drops the removed Box<HeapObject>s, freeing memory
}
```

### Where to mark/release: macro expansion boundary

Each macro expansion in `expand_macro_call_inner` (macro_expand.rs) follows
this pattern:

```
1. Build let-expression wrapping macro body with args  (allocates SyntaxLiteral Values)
2. eval_syntax(let_expr, ...) → result_value           (creates temp heap objects)
3. Syntax::from_value(&result_value, ...) → result_syntax  (converts to owned Syntax)
4. add intro scope → hygienized syntax
5. expand(hygienized, ...) → recurse on result
```

After step 3, `result_value` is dead — all data is in the `Syntax` tree as
owned Strings and Vecs. The temp heap objects from steps 1-2 are dead.

**Safe release point**: between step 3 and step 4.

```rust
fn expand_macro_call_inner(&mut self, ...) -> Result<Syntax, String> {
    let _guard = ArenaGuard::new();     // mark BEFORE let_expr construction
    
    // ... build let_expr (allocates SyntaxLiteral Values via alloc()) ...
    let result_value = eval_syntax(let_expr, self, symbols, vm)?;
    let result_syntax = Syntax::from_value(&result_value, symbols, span)?;
    
    drop(_guard);                       // explicit release; also drops on error/panic
    
    let intro_scope = self.fresh_scope();
    let hygienized = self.add_scope_recursive(result_syntax, intro_scope);
    self.expand(hygienized, symbols, vm)
}
```

The `ArenaGuard` uses RAII: if `eval_syntax` or `from_value` returns `Err`,
the guard drops during unwinding, releasing the arena. This prevents leak
accumulation on error paths.

### Safety argument

**Why this doesn't cause use-after-free:**

1. `from_value` converts Values to `SyntaxKind` variants with owned data:
   - Atoms (nil, bool, int, float): immediate Values, not heap pointers
   - Symbol: `SyntaxKind::Symbol(name.to_string())` — cloned String
   - Keyword: `SyntaxKind::Keyword(name.to_string())` — cloned String
   - String: `SyntaxKind::String(s)` — cloned String
   - List: recursive `from_value` on each cons cell element
   - Tuple/Array: cloned Vec, recursive `from_value`
   - Syntax object: `syntax_rc.as_ref().clone()` — deep clone of Syntax tree

   **No Value heap pointers survive in the output Syntax.**

2. `eval_syntax` returns a single `Value`. After `from_value` extracts its
   data, this Value is dead. The bytecode created inside `eval_syntax` is
   dropped when `eval_syntax` returns (it's a local variable).

3. Nested macro expansions (step 5 calls `expand` which may trigger more
   macro calls) each get their own `ArenaGuard`. Inner guards release before
   outer guards — proper stack discipline.

4. `HeapObject` has no custom `Drop` impl. When `Vec::truncate` drops Boxes,
   Rust's default drop walks fields. Fields that are Values are `Copy` (no-op
   drop). Fields that are `Rc<T>` decrement refcount and free if zero. No
   reentrant allocation happens during drop.

### Known unsoundness

`deref()` in `heap.rs` returns `&'static HeapObject`. Under the arena scheme,
this lifetime is a lie for objects allocated between mark and release — the
reference becomes dangling after release. This is safe in practice because:

- `deref` is called within accessor methods (`as_cons`, `as_closure`, etc.)
- These accessors return short-lived borrows used within a single expression
- No code path retains a `&HeapObject` reference across an arena release point
- The release point is in `expand_macro_call_inner`, not in any accessor

**This is not enforced by the type system.** It is an intentional unsoundness
that was already present (the `'static` lifetime was always a lie — the memory
is valid only because nothing ever freed it). The arena makes this concrete.

### What about `eval()` (caller's VM)?

`eval()` uses `get_cached_expander_and_meta()` and runs on the caller's VM.
Macro expansion creates temporaries on the thread-local arena. The
mark/release in `expand_macro_call_inner` frees these temporaries.

After expansion, `eval()` compiles and executes code. These allocations
(Bytecode constants, runtime Values) go into the arena but are OUTSIDE any
mark/release scope. They persist for the thread lifetime — same as today.

The result Value from `eval()` is returned to the caller. It may be a heap
pointer into the arena. It is never released. Same leak behavior as today
for non-expansion allocations.

### What about `vm/eval.rs::eval_inner` (runtime `eval`)?

This is the handler for the `eval` special form at runtime. It compiles and
executes a datum. It does NOT go through `expand_macro_call_inner` — it calls
`expander.expand()` directly. If the evaluated form contains macros, those
macros DO trigger `expand_macro_call_inner`, which DOES get arena mark/release.

So runtime `eval` benefits from the arena to the extent that macro expansion
within the evaluated form is covered. The compilation and execution allocations
are not released. This matches the intended behavior.

### Invariant

**Mark/release must ONLY be placed in `expand_macro_call_inner`.** Never around
compilation entry points (`compile`, `compile_all`, `eval`) or execution
(`vm.execute`). These return Values or Bytecodes containing Values that must
survive.

### What changes

| File | Change |
|------|--------|
| `src/value/heap.rs` | Replace `Rc::into_raw` in `alloc()` with arena push. Add `alloc_permanent()` using current `Rc::into_raw`. Add `heap_arena_mark()`, `heap_arena_release()`, `ArenaGuard`. |
| `src/value/repr/constructors.rs` | `Value::native_fn()` uses `alloc_permanent()` instead of `alloc()`. |
| `src/syntax/expand/macro_expand.rs` | Add `ArenaGuard` in `expand_macro_call_inner`, before `let_expr` construction, released after `from_value`. |

### What does NOT change

- `Value` remains `Copy`. No structural changes.
- `from_heap_ptr`, `deref`, `as_heap_ptr` — unchanged.
- `STRING_INTERNER` — unchanged, strings are still permanent.
- All Value constructors except `native_fn` — unchanged, still call `alloc()`.
- VM, Fiber, pipeline — unchanged.
- Tests, examples — unchanged.

### Pre-existing bug (separate issue)

`Syntax::from_value()` for structs and tables drops keys — only values are
converted. `struct_ref.iter().flat_map(|(_, v)| ...)` discards the key. This
is a data-loss bug in macro results that return structs/tables. Not related
to the arena, but discovered during review. File separately.

### Expected impact

Each macro expansion in `fn/graph` (3 nested `each` + `let*` + `->`) creates
~30-50 `eval_syntax` calls, each generating hundreds of temporary cons cells,
syntax objects, and closures. With mark/release, these are freed after each
expansion. Memory usage for repeated `define_graph_functions` calls becomes
bounded instead of linear.

- Before: 10,920 KB/iter for define_graph_functions
- After: ~constant (same temporaries created and freed each time)

### Testing

1. **Existing test suite must pass**: arena is transparent to correct code.
2. **h5k test**: `define_graph_functions` leak rate should drop to near-zero.
3. **New test**: verify mark/release frees memory (RSS before/after).
4. **Counterfactual**: temporarily remove release, verify leak returns.
5. **Error path test**: macro expansion that errors should still release arena.
</content>
