# The lowerer

<!-- audited: 2026-11-22 -->

How the lowerer turns HIR into LIR: each binding's slot, the capture cells, the inlined constants, and the blocks control flow becomes.

[The LIR module](../overview.md) owns the types the lowerer writes and the rules
every block keeps, and [the LIR design](../../../docs/impl/lir.md) owns the
working form the lowerer builds in. [The region instructions](regions.md) are
what the lowerer emits from the region solver's answer. The lowerer decides no
binding and no lifetime: the analyzer resolves every binding, and the solver
places every retain and release.

## Building a lowerer

`Lowerer::new` takes the analyzer's `&BindingArena`, so no binding's metadata
can change while lowering reads it. The `with_*` builders ([setup.rs](setup.rs))
hand it the rest of the front end's products before `lower` runs:

| Builder | What it hands the lowerer |
|---------|---------------------------|
| `with_heap` | The heap `lower` mints its working region on, and frees it from before returning, on the error path too |
| `with_region_info` | The solver's `RegionInfo`, indexed once by site ([order.rs](order.rs)) |
| `with_primitive_classification` | The intrinsic table and the declared native effects |
| `with_primitive_values` | The primitives' bindings and values, the first entries of the constant map |
| `with_type_info` | The inferred node types an operand proof reads ([the LIR design](../../../docs/impl/lir.md)) |
| `with_symbols` | The spellings an `undefined variable` error and a lambda's binder name read |

`lower` runs the whole-unit questions once before any body lowers: escape, the
bindings a closure captures, the tail calls a `Return` mints for, and the
bindings a pattern binds to a borrowed subview of its scrutinee
([aliases.rs](aliases.rs)). It answers a `FrozenModule`, whose closures are
numbered in pre-order: a lambda reserves its `ClosureId` before its body
lowers, so a parent's id is lower than its children's.

## Constants

`immutable_values` maps a binding to its compile-time value. A read of a
binding in the map lowers to `ValueConst`, and no slot is read. A primitive
never takes a slot at all.

- `with_primitive_values` seeds every primitive.
- `try_seed_immutable` seeds an immutable binding whose initializer is a
  literal, a quote, or a read of a binding already in the map.
- `lower_bind_value`, `lower_assign` and `lower_letrec` remove a binding as
  they store it again. A file-scope destructure can reuse an earlier binding's
  identity, and the eviction keeps the earlier constant out of its reads.

The map is not saved across a lambda boundary, so a lambda body inlines its
parent's constants. A captured constant still takes its capture slot, which the
body never reads.

## Slots and address spaces

`binding_to_slot` maps every binding a function reads to a `u16` slot. A read
that finds none fails with `undefined variable`, and an assignment with
`Unknown binding`: the HIR and the slots disagree.

One counter numbers every local of a function. Each parameter owns the local
slot of its index, so the first local past the parameters is slot `num_params`.
Inside a lambda two address spaces share that numbering:

| Space | Holds | Read and written by |
|-------|-------|---------------------|
| the environment | the captures, then every parameter, then one slot per local past the parameters | `LoadCapture`, `StoreCapture` |
| the stack | each local and each parameter no cell holds | `LoadLocal`, `StoreLocal` |

`allocate_slot` answers the environment index `num_captures + local` for a
local that needs a cell, and the stack slot `local` for any other.
`upvalue_bindings` names the bindings read from the environment: the captures,
the parameters a cell holds, and the locals that need a cell. Outside a lambda
every binding is a stack slot. `value_slot_for` ([regionemit/slots.rs](regionemit/slots.rs))
is the one place that re-derives the space, so a site that records a slot cannot
disagree with the allocation. The emitter reads an environment index past the
parameters whose `capture_locals_mask` bit is clear as that local's stack slot.
The JIT and the WASM backend give each stack slot a variable of their own.

Two masks on the function's header say which slots hold a cell:

- **`capture_params_mask`** sets bit `i` when parameter `i` needs a cell at
  call time. Only an `@` parameter can be mutated, so the mask is usually 0. The
  analyzer refuses a function with more than 64 parameters
  ([functions](../../../docs/functions.md)), so every parameter has a bit.
- **`capture_locals_mask`** sets bit `i` when local `i` past the parameters
  needs an environment cell, because a closure captures it or `assign`
  mutates it. It is a `CaptureMask` ([capturemask.rs](../../value/capturemask.rs)),
  unbounded in width. The environment builders of the VM, the JIT and the WASM
  backend read it, and give every other local a bare `nil` slot.

## Capture cells

A binding a closure captures lives in a cell, and one of three sites makes it:

| Cell | Made by | Lives in |
|------|---------|----------|
| the environment cell | the tier that builds the environment, as it builds it (`populate_env` in the VM) | the environment slot `allocate_slot` answered |
| the forward cell | `allocate_compiled_cell_slot` ([emitops.rs](emitops.rs)), from the pre-pass of `lower_letrec` and of `lower_begin` | the binding's own stack slot |
| a `let` cell outside a lambda | `lower_let` ([binding/let.rs](binding/let.rs)) | the binding's own stack slot |

A forward cell is the cell a sibling closure captures before the binding's
initializer runs (`BindingInner::compiled_forward_cell`). The one constructor
mints the cell and records the binding in `compiled_cell_bindings`, so neither
can happen without the other. A binding in that set is not an upvalue: it reads
`LoadLocal`, then `LoadCaptureCell`. Nothing captures a `let` binding before its
initializer runs, so `lower_let` does not record its cell.

Every binder that stores an initializer into a cell does so through
`store_captured_cell_init` or `store_env_cell_init` ([regionemit/cellstore.rs](regionemit/cellstore.rs)).

## Control flow

Each branch form stores each arm's value into one result slot and reloads it
at the merge block. A form that jumps works the same way:

| Form | Lowers to |
|------|-----------|
| `block` | a result slot and an exit label, on `block_lower_contexts` while the body lowers |
| `break` | a store into its block's result slot, a jump to the exit label, and a dead block after it |
| `loop` | the binding slots and a header label, on `loop_lower_contexts` while the body lowers |
| `recur` | a store into each binding slot, a jump to the header, and a dead block after it |

No form here needs an opcode of its own: each is a store and a jump. The dead
block after a `break` or a `recur` answers `nil`, because the arm that jumped
can still store its value into an enclosing form's result slot there. What a
`break` owes the regions it jumps over is in
[the region instructions](regions.md).

## The header

`lower_lambda_body` ([lambda/body.rs](lambda/body.rs)) fills the lambda's
`LirHead` from the analyzer's `Lambda`: its arity, captures, parameters, signal,
docstring and origin. The docstring rides to the code payload, where
`ClosureTemplate::doc()` reads it, and no bytecode carries it.

## Files

| File | What it lowers or decides |
|------|---------------------------|
| [mod.rs](mod.rs) | The `Lowerer`, the state one function's lowering carries, and `lower` |
| [setup.rs](setup.rs) | The constructor and the `with_*` builders |
| [emitops.rs](emitops.rs) | Registers, slots, instruction emission and blocks |
| [expr.rs](expr.rs) and [expr/](expr/) | The `lower_expr` dispatch, then each expression form |
| [binding.rs](binding.rs) and [binding/](binding/) | `let`, `letrec`, `def`, `assign` and destructuring |
| [lambda.rs](lambda.rs) and [lambda/](lambda/) | Closure construction, and the body as a function of its own |
| [control.rs](control.rs) and [control/](control/) | `and`, `or`, `match`, `emit`, `eval`, and calls |
| [pattern.rs](pattern.rs) and [pattern/](pattern/) | A decision tree lowered to blocks, and the patterns a suspending guard lowers in sequence |
| [access.rs](access.rs) | Loading a value along a pattern's access path |
| [aliases.rs](aliases.rs) | The bindings a pattern binds to a borrowed subview of its scrutinee |
| [tailcall.rs](tailcall.rs) | Whether a body ends in a tail call |
| [rcstats.rs](rcstats.rs) | How many mints and releases resolved to a static slot |

The files that emit region instructions are listed in
[the region instructions](regions.md).

A new special form takes an arm in `lower_expr` and a method of its own. A
change to where a region is released, or to which regions merge, belongs in the
solver ([infer.rs](../../hir/region/infer.rs)), never here.
