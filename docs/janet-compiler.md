# Janet's Compiler Architecture

A design reference for language designers. Janet's compiler is a single-pass
tree-walking compiler that produces bytecode for a register-based VM. It is
notable for its tight integration with the runtime (macros trigger VM
execution during compilation), its unified treatment of special forms as
pluggable code generators, and its use of the environment table as both
compile-time symbol table and runtime data structure.


## Overview

The compilation pipeline:

```
source text → parser → AST (Janet values) → compiler → JanetFuncDef → wrap in JanetFunction → execute
```

The compiler takes a single Janet value (the AST) and an environment table,
and produces a `FuncDef` — a bytecode function definition containing
instructions, constants, source maps, nested function definitions, and debug
info. The top-level compilation always produces a zero-argument "thunk" that,
when called, executes the top-level forms and populates the environment.

Compilation and execution are **mutually recursive**: the compiler invokes the
VM to expand macros, and the VM can invoke the compiler via `eval`/`compile`.
There is no strict phase separation.


## The Compiler State

```
JanetCompiler:
  scope        Scope*              Current scope (head of scope chain)
  buffer       uint32_t*           Shared bytecode output buffer
  mapbuffer    SourceMapping*      Source map parallel to bytecode buffer
  env          Table*              Compile-time environment table
  source       string              Source filename
  result       CompileResult       Output: status, error, funcdef, error position
  current_mapping  SourceMapping   Current source position (line, column)
  recursion_guard  int             Prevents unbounded macro/compile recursion
  lints        Array*              Optional lint message accumulator
```

**Key design decision**: There is a single shared bytecode buffer. When a
nested function scope is popped, the bytecode from that scope's start offset
to the end of the buffer is sliced out and copied into the new `FuncDef`. The
buffer is then truncated back. This avoids per-function buffer allocation at
the cost of a copy on function completion.


## Scopes

Scopes form a linked list via `parent`/`child` pointers. Each scope holds:

```
JanetScope:
  name             string          Debug name ("root", "if", "while", "function")
  parent           Scope*          Enclosing scope
  syms             SymPair[]       Symbol table: linear array of (symbol, slot) pairs
  consts           Value[]         Constants array (for this funcdef)
  defs             FuncDef*[]      Nested function definitions
  envs             EnvRef[]        Referenced closure environments
  ra               RegisterAlloc   Register allocator
  ua               RegisterAlloc   Upvalue allocator (tracks captured registers)
  bytecode_start   int32           Index into shared buffer where this scope's code begins
  flags            int             FUNCTION, ENV, TOP, UNUSED, CLOSURE, WHILE
```

### Scope Flags

| Flag | Meaning |
|------|---------|
| `FUNCTION` | This scope is a function boundary (new funcdef, new register file) |
| `ENV` | This scope needs a closure environment (has upvalues captured from it) |
| `TOP` | Top-level scope (suppresses tail calls for better error messages) |
| `UNUSED` | Throwaway scope for dead code (syntax-checked but no bytecode emitted) |
| `CLOSURE` | A closure was created inside this scope (matters for while-loop rewriting) |
| `WHILE` | This is a while-loop scope (target for `break`) |

### Scope Lifetime

Non-function scopes (`do`, `if`, `while`) **share the parent's register
file** — the register allocator is cloned from the parent. Registers allocated
in the child are freed when the child scope is popped, but the high-water mark
propagates upward.

Function scopes start with a fresh register allocator. When popped via
`pop_funcdef()`, the scope's bytecode, constants, environments, and nested
defs are extracted into a new `FuncDef`.


## Symbol Resolution

When the compiler encounters a symbol, `resolve()` runs a three-phase lookup:

### Phase 1 — Lexical Scope Walk

Walk the scope chain from current to root. For each scope, search its `syms`
array in **reverse order** (later bindings shadow earlier ones). If found:

- **Same function scope**: It is a local — return the register slot directly.
- **Across a function boundary**: It is an upvalue — trigger closure
  environment setup (Phase 3).

### Phase 2 — Environment Lookup

If not found in any scope, look up the symbol in `compiler->env` (the
compile-time environment table). The environment maps symbols to **entry
tables** — small tables with specific keys that describe the binding:

| Binding Type | Entry Table Shape | Compiler Behavior |
|---|---|---|
| `def` | `{:value x}` | Inline `x` as a constant (zero runtime cost) |
| `var` | `{:ref @[x]}` | Emit indirection through the ref array |
| `macro` | `{:value f :macro true}` | Should have been expanded already |
| `dynamic def` | `{:ref @[x] :redef true}` | Emit indirection through ref array |
| `dynamic macro` | `{:ref @[f] :macro true :redef true}` | Deref ref array for macro function |

For `def` bindings, the value is embedded directly as a constant in the
bytecode. This means global `def` lookups have **zero runtime cost** — the
value is baked into the function definition at compile time.

For `var` bindings, the compiler emits `GET_INDEX ref, 0` to read and
`PUT_INDEX ref, 0, value` to write. The ref array is the indirection layer
that allows mutation. Multiple compilation units sharing the same ref array
see each other's mutations.

### Phase 3 — Upvalue Propagation

If a symbol was found in a lexical scope across a function boundary:

1. Mark the binding's slot as `keep = 1` (prevent register reuse after scope pop)
2. Walk back to the owning function scope, mark it `ENV` (needs closure env)
3. Register the slot in the upvalue allocator
4. For each function boundary between owner and current scope, set up an
   environment reference chain (`EnvRef` entries)
5. Return a slot with `envindex >= 0` (upvalue, not local)

### Missing Symbol Handler

If the symbol is not found anywhere, the compiler checks for a
`:missing-symbol` key in the environment. If it is a function, the compiler
**calls it via the VM** — this is another point where compilation triggers
runtime execution. The handler can return a binding or signal an error.


## The Entry Table Protocol

Every binding in the environment is represented as a table with specific keys:

| Key | Type | Purpose |
|-----|------|---------|
| `:value` | any | The bound value (for `def` bindings) |
| `:ref` | array | A 1-element array for mutable/redefinable bindings |
| `:macro` | truthy | Marks the binding as a macro |
| `:redef` | truthy | Marks a def as redefinable (uses ref indirection) |
| `:doc` | string | Documentation string |
| `:source-map` | tuple | `[source line column]` of the definition |
| `:deprecated` | keyword/truthy | Deprecation level: `:relaxed`, `:normal`, `:strict` |
| `:private` | truthy | Not exported from modules (handled in Janet, not C) |

The binding type is determined by a classification matrix:

| `:macro`? | `:ref`? | `:redef`? | Result |
|-----------|---------|-----------|--------|
| no | no | — | `DEF` (constant, inlined) |
| no | yes | no | `VAR` (mutable, ref indirection) |
| no | yes | yes | `DYNAMIC_DEF` (redefinable, ref indirection) |
| yes | no | — | `MACRO` (expanded at compile time) |
| yes | yes | yes | `DYNAMIC_MACRO` (redefinable macro) |

**Design note**: The entry table is a plain hash table, not a struct. This
means bindings can carry arbitrary metadata (any keyword becomes a key). The
cost is runtime type-checking on every lookup. The benefit is extensibility —
linters, documentation tools, and IDE support can attach metadata without
changing the binding protocol.


## Special Forms

Special forms are C function pointers in a static sorted array, dispatched by
binary search on the symbol name. There are exactly 13:

```
break  def  do  fn  if  quasiquote  quote  set  splice  unquote  upscope  var  while
```

Each handler has the signature:

```
Slot handler(FuncOpts opts, int32_t argn, const Value *argv)
```

Where `opts` provides:
- `opts.compiler` — full compiler state (scope chain, env, buffer)
- `opts.flags` — `TAIL` (tail position), `HINT` (preferred target register),
  `DROP` (result unused), `ACCEPT_SPLICE`
- `opts.hint` — suggested target slot for register allocation efficiency

Special forms are **purely compile-time**. They emit bytecode, create/pop
scopes, recursively compile sub-expressions, and register symbols. They never
execute code (that is the VM's job). They never see the VM directly.

**Dispatch priority**: Special form lookup happens **before** macro lookup.
If a symbol names a special form, it is never treated as a macro. This means
the 13 special forms are truly reserved — no macro can shadow them.

### `def` — The Interesting One

At **top-level scope**, `def` does both compile-time and runtime work:

1. **Compile-time**: Creates an entry table `{:value nil}` and immediately
   adds it to `compiler->env`. This is how later forms in the same
   compilation unit can see the binding before the thunk runs.

2. **Runtime**: Emits `PUT entry_table, :value, computed_value` — bytecode
   that stores the actual computed value into the entry table when the thunk
   executes.

The entry table is embedded as a constant in the funcdef. So the environment
has the entry at compile time (with `:value` initially nil), and the thunk
fills in the real value at runtime.

At **inner scopes**, `def` just registers a symbol-to-register mapping in the
scope's `syms` array. No entry table, no environment mutation. Pure register
allocation.

### `var` — Mutable Bindings

At top-level: creates an entry table with `:ref @[nil]`, adds it to the env,
emits `PUT_INDEX ref, 0, value` to store the computed value through the ref
array.

At inner scopes: allocates a register slot with the `MUTABLE` flag. The `set`
special form checks this flag — only mutable slots can be assigned to.

### `if` — Conditional

Bytecode pattern:

```
[compile condition]
JUMP_IF_NOT cond → right
[compile true branch]
JUMP → done          (omitted in tail position)
right:
[compile false branch]
done:
```

Optimizations:
- **Nil-check detection**: If the condition is `(= nil x)` or `(not= nil x)`,
  emits `JUMP_IF_NIL` / `JUMP_IF_NOT_NIL` instead of a general truthiness
  check.
- **Constant folding**: If the condition is a compile-time constant, only the
  live branch is compiled. The dead branch is syntax-checked but discarded
  (with a lint warning).
- **Tail position**: Both branches are compiled with the tail flag, so each
  emits its own return. No jump-to-done needed.

### `while` — Loops

Bytecode pattern:

```
top:
[compile condition]
JUMP_IF_NOT cond → done
[compile body (result dropped)]
JUMP → top
done:
```

Optimizations:
- **Constant-true condition**: Skips the condition check entirely (infinite
  loop, only `break` exits).
- **Nil-check detection**: Same as `if`.

**`break` interaction**: `break` emits a tagged jump (`0x80 | JUMP`) as a
sentinel. After the loop body is compiled, the compiler scans for these
sentinels and patches them to jump to the loop exit. This is a simple
two-pass approach that avoids maintaining a break-target stack.

**Closure-in-loop rewriting**: If a closure is created inside a while loop
(the `CLOSURE` flag propagates up to the `WHILE` scope), the entire loop is
recompiled as a **tail-recursive IIFE**. The loop body becomes a function
that tail-calls itself. This is necessary because closures over loop variables
would otherwise capture the register, which is mutated on each iteration.

### `fn` — Function Definition

1. Opens a new `FUNCTION` scope
2. Processes parameters left to right:
   - Plain symbols → allocate registers, name them
   - `&` → mark vararg position
   - `&opt` → set minimum arity
   - `&keys` → collect remaining args as a struct
   - `&named` → named argument destructuring with a lookup table
   - Non-symbol patterns → defer destructuring until after all params
3. Compiles body forms (last in tail position)
4. Pops the function scope via `pop_funcdef()`:
   - Extracts bytecode slice from shared buffer
   - Copies constants, environments, nested defs
   - Builds closure bitset and symbol map
   - Runs optimization passes
5. Emits `CLOSURE` instruction to instantiate the function

### `do` — Sequencing

Compiles each sub-form in order. All but the last have `DROP` flag (result
unused). The last inherits the parent's flags (including `TAIL`).

### `set` — Assignment

Compiles the value with the target slot as a hint (so the result lands
directly in the right register). Only works on `MUTABLE` slots. For ref
slots, emits `PUT_INDEX`.

### `upscope` — Scope Flattening

Compiles sub-forms **without** creating a new scope. Bindings created inside
`upscope` are visible in the enclosing scope. Used by macros that want to
inject bindings into the caller's scope.

### `quote` / `quasiquote` / `unquote` / `splice`

`quote` returns its argument as a constant. `quasiquote` walks the tree,
compiling `unquote` forms and splicing `splice` forms, building the result
at runtime via array/tuple construction instructions.


## Macros

When the compiler encounters a tuple `(sym ...)` where `sym` is not a special
form, it checks the environment for a macro binding.

### Expansion

1. Look up `sym` in `compiler->env`
2. If the binding type is `MACRO` or `DYNAMIC_MACRO` and the value is a
   function:
   - Create a fiber: `fiber_new(macro_fn, arity, form_args)`
   - Set `fiber->env = compiler->env` (macro sees the compile-time env)
   - Store `:macro-form` in the env (so the macro can access the raw form)
   - **Call into the VM**: `continue(fiber, nil, &output)`
   - The output replaces the original form
3. Repeat up to 200 times (macros can expand to other macros)

### Implications

- Macros run **during compilation**, with full VM access
- Macros can read and write the compile-time environment
- Macros can perform I/O, allocate memory, call other functions
- Macros receive the form's arguments as unevaluated Janet values (AST)
- The GC is locked during macro execution to prevent collection of
  compiler-owned data

**Design note**: This is the Lisp tradition — macros are functions that
transform syntax trees, executed at compile time. The cost is that compilation
is not pure — it can have side effects. The benefit is maximum
metaprogramming power with no separate macro language.


## The Optimizer / Inliner System

When the compiler encounters a function call where the function is a
compile-time constant, it checks for an **optimizer tag** on the function
definition.

### How It Works

1. Built-in functions are created with a tag in their `FuncDef.flags`
   (e.g., `JANET_FUN_ADD = 9`)
2. At a call site, the compiler extracts the tag and indexes into a static
   `optimizers[]` array
3. Each optimizer entry has:
   - `can_optimize(opts, args)` — arity/validity check (NULL = always valid)
   - `optimize(opts, args)` — emits specialized bytecode, returns result slot

### Optimized Functions

| Tag | Function | Emits |
|-----|----------|-------|
| 1 | `debug` | `SIGNAL` (debug) |
| 2 | `error` | `ERROR` |
| 3 | `apply` | `PUSH_ARRAY` + `TAILCALL`/`CALL` |
| 4 | `yield` | `SIGNAL` (yield) |
| 5 | `resume` | `RESUME` |
| 6 | `in` | `IN` |
| 7 | `put` | `PUT` |
| 8 | `length` | `LENGTH` |
| 9–12 | `+` `-` `*` `/` | Arithmetic opcodes (with immediate variants) |
| 13–18 | `band` `bor` `bxor` `blshift` `brshift` `brushift` | Bitwise opcodes |
| 19 | `bnot` | `BNOT` |
| 20–25 | `>` `<` `>=` `<=` `=` `not=` | Comparison opcodes (short-circuit chain) |
| 26 | `propagate` | `PROPAGATE` |
| 27 | `get` | `GET` |
| 28 | `next` | `NEXT` |
| 29–30 | `mod` `%` | Modulo/remainder opcodes |
| 31 | `cmp` | `CMP` |
| 32 | `cancel` | `CANCEL` |
| 33 | `div` | Floor division opcode |

### Variadic Arithmetic (`opreduce`)

For operators like `+`, which accept any number of arguments:

- **0 args**: Return the identity constant (`0` for `+`, `1` for `*`)
- **1 arg**: Apply the operator to the identity and the argument
- **2+ args**: Emit a left-fold chain: `t = a op b# t = t op c# ...`
- **Immediate optimization**: If an argument is a small constant (fits in
  int8), use the immediate-operand instruction variant (e.g.,
  `ADD_IMMEDIATE`) to avoid loading a constant into a register

### Comparison Chains (`compreduce`)

For operators like `<`, which can be chained (`(< a b c)` means
`a < b` and `b < c`):

- Emit a short-circuit chain: each comparison jumps to the end on failure
- The result is a boolean in the target register

**Design note**: The optimizer is **not** a special form. `+` is a real
function in the environment. The optimization is purely opportunistic, keyed
off a tag in the function definition. If you rebind `+` to a different
function, the tag is lost and calls become normal function calls. This is
clean — no special-casing in the language, just a tag on the function
definition. But it is a closed set: user code cannot add new optimizer tags.


## The Register Allocator

A first-fit bitset allocator. Each function scope has its own allocator.

```
RegisterAllocator:
  chunks     uint32_t[]    Bitset (1 bit per register, 1 = allocated)
  max        int32         Highest register ever allocated
  regtemps   uint32        Bitmask of temp registers in use
```

### Allocation

Scan chunks linearly for the first 0 bit (using `__builtin_ctz` on the
complement). Set the bit, update `max`, return the register index.

### Temporary Registers

Registers 240–255 are reserved for temporaries. When an instruction needs a
near register (8-bit field) but the allocator returns a far register (>= 256),
the emitter falls back to the reserved range and emits move instructions to
shuttle values in and out.

### The Hint System

`FuncOpts` carries a `hint` slot — a suggested target register. When the hint
is a valid local register, `gettarget()` returns it directly instead of
allocating a new one. This avoids unnecessary move instructions.

The `set` special form uses this: the assignment target is passed as the hint
to the value compilation, so the result is written directly to the correct
register. After compilation, if the result ended up elsewhere, a single `COPY`
is emitted.


## The Slot Abstraction

`JanetSlot` is the compiler's representation of a value location:

```
JanetSlot:
  constant   Value       Compile-time constant value (if CONSTANT flag set)
  index      int32       Register index (-1 for constants)
  envindex   int32       -1 for local, >= 0 for upvalue (environment index)
  flags      uint32      See below
```

### Slot Flags

| Flag | Meaning |
|------|---------|
| `CONSTANT` | Value is known at compile time. Not backed by a register. |
| `NAMED` | Slot has a symbol binding. Register is not freed when slot is freed. |
| `MUTABLE` | Slot is a `var`. Only mutable slots can be targets of `set`. |
| `REF` | Indirect reference through a 1-element array. Reads emit `GET_INDEX 0`# writes emit `PUT_INDEX 0`. |
| `RETURNED` | A return/tailcall was already emitted for this slot. Prevents double-return. |
| `SPLICED` | Value should be spliced (pushed as array) during function calls. |

The low 16 bits are a **type mask** — one bit per Janet type — used for
compile-time type checking and lint warnings.

### How `REF` Works

For global `var` bindings, the slot's `constant` field holds the ref array
(a 1-element Janet array). To read:

1. Load the ref array constant into a register
2. Emit `GET_INDEX register, 0`

To write:

1. Load the ref array constant into a temp register
2. Emit `PUT_INDEX temp, 0, value_register`

This indirection is what makes global vars mutable across compilation units.
The ref array is the single source of truth# the entry table in the
environment points to it, and all compiled code accesses through it.


## Function Call Compilation

When the compiler encounters a non-special, non-macro call `(f a b c)`:

### Step 1 — Try Specialization

If `f` is a compile-time constant function with an optimizer tag, and there
are no spliced arguments: call the optimizer. This replaces the entire call
with specialized bytecode (e.g., `ADD` instead of `CALL`).

### Step 2 — Compile-Time Arity Checking

If `f` is a known constant function, the compiler validates:
- Argument count against min/max arity
- Odd argument count for `&keys`/`&named` functions
- Named argument keys against the function's declared named parameters

These are compile-time warnings/errors, not runtime checks.

### Step 3 — Push Arguments

Arguments are pushed onto the fiber's argument stack via:
- `PUSH` (1 arg), `PUSH_2` (2 args), `PUSH_3` (3 args)
- `PUSH_ARRAY` for spliced arguments

### Step 4 — Emit Call

- In tail position (and not top scope): emit `TAILCALL`
- Otherwise: emit `CALL` with a target register for the result


## Source Mapping

Every bytecode instruction has a parallel `SourceMapping` entry (line, column).
The compiler maintains `current_mapping`, updated whenever it encounters a
tuple with source annotations (attached by the parser).

When `pop_funcdef()` extracts bytecode, it also extracts the corresponding
source map slice. The optimization pass that removes noops compacts the source
map in lockstep.

### Symbol Map (Debug Info)

Beyond instruction-level source maps, the compiler builds a symbol map:

```
SymbolMap:
  birth_pc    uint32     First instruction where this binding is live
  death_pc    uint32     Last instruction where this binding is live
  slot_index  uint32     Register or upvalue index
  symbol      string     Binding name
```

This enables debuggers and stack traces to show local variable names and
values for any given program counter.


## The Bootstrap Process

Janet's core environment is built in two modes:

### Bootstrap Mode (Build Time)

1. Create an empty environment table
2. Register hand-assembled bytecode functions (`+`, `-`, `error`, `yield`,
   etc.) via `quick_asm()` — these are tiny functions with optimizer tags
3. Register all C functions from all modules (io, math, array, string, etc.)
4. Run `boot.janet` — this defines all the Janet-level standard library
   (macros, higher-order functions, module system, etc.)
5. Marshal the entire environment into a binary image

### Normal Mode (Runtime)

1. Build a lookup table mapping C function pointers to names
2. Unmarshal the pre-built binary image using the lookup table to resolve
   C function references
3. The result is the complete core environment, ready to use

This means the core environment is built once at build time and loaded from a
binary blob at runtime. The lookup table bridges the gap between the
marshaled image (which cannot contain raw C pointers) and the running process.

**Design note**: This is a clean bootstrap design. The binary image contains
all Janet-defined functions as bytecode, but C functions are referenced by
name and resolved at load time. This means the image is portable across
builds (as long as the C function set is compatible).


## Design Principles to Extract

### 1. The Environment Is the Symbol Table

There is no separate symbol table data structure. The compile-time environment
is a plain hash table mapping symbols to entry tables. This means:
- The "symbol table" is a runtime-mutable data structure
- Macros can inspect and modify bindings during compilation
- The same table serves as both compile-time symbol table and runtime
  module interface
- The cost is runtime type-checking on every lookup

### 2. Entry Tables Over Binding Structs

Bindings are represented as tables with keyword keys, not as typed structs.
This is extensible — any tool can attach metadata to a binding without
changing the binding protocol. Documentation, deprecation warnings, source
locations, and user-defined metadata all live in the same table.

The trade-off: every binding lookup involves hash table access and key
checking. A typed struct would be faster but less extensible.

### 3. Constants Are Free

`def` bindings are inlined as constants at compile time. The value is baked
into the funcdef's constant table. There is no runtime lookup, no indirection,
no hash table access. This is the payoff of the entry table protocol — the
compiler can distinguish `def` (constant, inline it) from `var` (mutable,
use ref indirection) at compile time.

### 4. Vars Use Ref Indirection

Mutable globals use a 1-element array as an indirection layer. This is simple,
GC-friendly, and allows multiple compilation units to share the same mutable
binding. The cost is one pointer chase per access. The alternative (a global
variable table with integer indices) would be faster but harder to manage
across dynamic module loading.

### 5. Optimizer Tags Over Special Forms

Built-in functions like `+` are real functions with a tag. The compiler
opportunistically inlines them at call sites. This is cleaner than making
arithmetic a special form:
- `+` can be passed as a value, stored in data structures, used as a
  higher-order function
- The optimization is transparent — it produces the same result as a call
- Rebinding `+` disables the optimization (correct, if surprising)

### 6. Macros Are Just Functions

No separate macro language. Macros are Janet functions that run during
compilation with full VM access. The compiler creates a fiber, resumes it,
and uses the output. This is maximum power with minimum mechanism.

The cost: compilation is not pure. A macro can do anything. The benefit:
no artificial restrictions on what macros can compute.

### 7. The Shared Buffer Pattern

Using a single bytecode buffer for the entire compilation, with nested
functions sliced out on scope pop, avoids per-function buffer allocation.
The trade-off is a copy when each function is finalized. For typical function
sizes, this is negligible.

### 8. Closure-in-Loop Rewriting

When the compiler detects a closure created inside a while loop, it rewrites
the loop as a tail-recursive IIFE. This is a correctness fix, not an
optimization — without it, closures would capture mutable loop variables by
reference, leading to the classic "all closures see the last value" bug.

This is a compile-time solution to a problem that many languages solve at
runtime (e.g., Python's late binding) or not at all (e.g., JavaScript's
classic `var` in loops). It is correct and automatic, but the rewrite changes
the performance characteristics of the loop.

### 9. Two-Pass Break Resolution

`break` emits a tagged jump sentinel. After the loop body, the compiler scans
for sentinels and patches them. This avoids maintaining a break-target stack
or forward-reference list. Simple, correct, and the scan is bounded by the
loop body size.

### 10. Mutual Recursion Between Compiler and VM

The compiler can invoke the VM (macros, missing-symbol handlers). The VM can
invoke the compiler (`eval`, `compile`). This is powerful but means neither
can be reasoned about in isolation. For a language design, this is a
fundamental choice: do you want a pure compiler with a separate macro system,
or a unified system where compilation and execution are interleaved?

Janet chooses interleaving. The payoff is simplicity — one language, one
evaluation model, one environment. The cost is that compilation can diverge,
have side effects, or fail in ways that depend on runtime state.
