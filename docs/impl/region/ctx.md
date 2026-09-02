# NativeCtx — explicit allocation: every value names its region and heap

Implementation-facing. Native code allocates only through a capability it is
handed. The `PrimFn` signature carries a `&mut NativeCtx`; that ctx owns the
call's region and carries heap access, so a primitive that allocates must use the
capability, and a primitive handed no other region cannot allocate anywhere but
its own call's region.

Correctness context: an allocation is sound only if it is born in the region the
solver assigned it (Rules 1 and 3, [rules.md](rules.md)). A
capability that names the region at every allocation site makes "allocate into
whatever region happens to be around" unrepresentable — and with it the two
defect classes such ambiguity generates: a native allocating into a region its
caller already released, and allocations piling into a catch-all region whose
release never fires.

## The contract

1. **Every allocation names its destination.** An allocation names its region
   and heap explicitly, or it does not compile.

2. **The only allocation entry points take an explicit region:**
   - `ctx.alloc(obj)` / `ctx.alloc_slice(items)` and the ergonomic `ctx.*`
     constructors (below) — a native's fresh allocations, into the ctx's own
     region;
   - `arena::alloc_in_region(obj, region)` and its `RegionSlice` twin — the
     compiler's per-slot allocation (`MaterializeConst`, the env builder, literal
     materialization), which resolves a `RuntimeRegion` directly.

   A new allocation call site names its region through one of these or it does
   not compile.

3. **Natives allocate through `NativeCtx`.** The `PrimFn` type is

   ```rust
   pub type PrimFn = fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value);
   ```

   A primitive's fresh allocations go to `ctx`. A primitive cannot allocate into
   any region but its own call's without being handed another explicitly — which
   nothing does.

## The capability split: `Alloc` vs `NativeCtx` (`src/primitives/ctx.rs`)

The capability comes in two layers, because two different callers need it:

- **`Alloc<'h>`** — region + heap + the `ctx.*` constructor surface, and nothing
  else. A *pure allocation context*: it structurally cannot re-enter the
  interpreter. Built at the sites that allocate Elle values with **no VM in
  scope** — the reader (`read_str`, run by the formatter/LSP), the async-IO
  completion payload builders (`io::io_error` and the backends' result values,
  born off the reaping call and handed to the resumed fiber), `send`
  reconstruction, the plugin `make_*` ctors, FFI argument marshalling, and test
  scaffolding.
- **`NativeCtx<'h>`** = `Alloc` + a **non-null** `*mut VM`. `Deref<Target =
  Alloc>` so every `ctx.string(..)`/`ctx.error(..)`/`ctx.alloc(..)` keeps working
  unchanged. Adds `ctx.vm() -> &mut VM`, which is **total** — never `Option`. A
  native runs only while a VM drives it, so the VM is never absent at a `vm()`
  call. Built **only** at the sites that have a VM: bytecode dispatch
  (`dispatch_native_call`), the WASM hosts (`call_primitive` / the tiered linker's
  `rt_call`), and trait-method dispatch (`call_method_fn`, from the caller's ctx).

```rust
/// Pure allocation capability: the call's own result region plus heap access.
/// No `vm()` — it cannot re-enter the interpreter.
pub struct Alloc<'h> {
    region: RuntimeRegion,
    heap: *mut FiberHeap,           // raw, guarded by the phantom borrow below
    _heap: PhantomData<&'h mut FiberHeap>,
}

impl<'h> Alloc<'h> {
    /// Build over an EXPLICIT, caller-resolved region — the bytecode dispatch
    /// constructor. The region is the solver's per-call result slot, resolved
    /// by `new_runtime_region_for_call_slot`; the ctx does NOT mint it.
    pub(crate) fn with_region(region: RuntimeRegion, heap: &'h mut FiberHeap) -> Self;
    /// Mint a fresh result region and own it — `boundary` is its alias for the
    /// call boundaries that have no compiler-assigned result slot.
    pub(crate) fn new(heap: &'h mut FiberHeap) -> Self;
    pub(crate) fn boundary(heap: &'h mut FiberHeap) -> Self;
    pub fn alloc(&self, obj: HeapObject) -> Value;
    pub fn alloc_slice<T: Copy + 'static>(&self, items: &[T]) -> RegionSlice<T>;
    // ergonomic constructors: ctx.string(s), ctx.pair(a, b), ctx.syntax(s), …
}

/// The native-call capability: `Alloc` plus a non-null VM.
pub struct NativeCtx<'h> {
    alloc: Alloc<'h>,
    vm: *mut VM,                    // non-null; guarded by the phantom borrow
    _vm: PhantomData<&'h mut VM>,
}
impl<'h> Deref for NativeCtx<'h> { type Target = Alloc<'h>; /* &self.alloc */ }
impl<'h> DerefMut for NativeCtx<'h> { /* &mut self.alloc */ }

impl<'h> NativeCtx<'h> {
    /// The three VM-bearing dispatch constructors mirror the `Alloc` ctors but
    /// also take the driving VM (`with_region_vm`, `new_vm`, `boundary_vm`).
    pub(crate) fn with_region_vm(region: RuntimeRegion, heap: &'h mut FiberHeap, vm: *mut VM) -> Self;
    pub(crate) fn boundary_vm(vm: &'h mut VM) -> Self;   // mints from vm.heap()
    /// Total: a native always runs under a VM. Reborrows the raw pointer (the
    /// aliasing contract: the VM does not touch itself during the synchronous
    /// call — the same contract `Alloc`'s heap pointer already relies on).
    pub fn vm(&self) -> &mut VM;
}
```

Why the split and not a nullable `vm`: a `NativeCtx` with `vm: Option<*mut VM>`
manufactures a `None` that never legitimately occurs at a `vm()` call, and
re-imports the very uncertainty the capability split removes. With the split,
*null is unrepresentable, not merely unused* — an allocation-only site holds an
`Alloc`, which has no `vm()` to call. The migration is compiler-driven: a helper
the compiler flags as reached from an allocation-only site changes `&mut NativeCtx`
→ `&mut Alloc` (always safe — `&mut NativeCtx` deref-coerces to `&mut Alloc`, so
native callers are unaffected); a helper that calls `ctx.vm()` stays `&mut
NativeCtx`, and is by construction never reached from an allocation-only site. The
`PrimFn` type is unchanged (`fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)`),
so no primitive body churns.

Invariants:

- The ctx owns its region for the duration of the call. On the bytecode
  dispatch path that region is the solver's per-call slot, minted by
  `new_runtime_region_for_call_slot` and handed in via `with_region_vm`; on a call
  *boundary* with no such slot (`new`/`boundary`/`boundary_vm`) the ctx mints a
  fresh one. The pass-through retain, the declaration oracle
  ([effects.md](effects.md)), and the caller's value-based release
  of the result are unchanged.
- `dispatch_query` (the in-dispatch `SIG_QUERY` answer) builds its answer through
  the same ctx, preserving "the answer is born in the call's own region"
  (built at the dispatch site, `vm/core/region.rs`).
- The JIT's `elle_jit_call` / `elle_jit_tail_call` route through
  `VM::dispatch_native_call`, so both tiers share its single bytecode-dispatch
  ctx construction and get identical region accounting for free.

### A helper reached from inside a call allocates through THAT call's ctx

`new`/`boundary` mint a region the *caller* has no name for, which is right at a
call boundary — the result escapes across an ABI and the consumer releases it by
value — and wrong anywhere inside a call that already owns a region. A helper
that mints its own region while building part of a native's result strands that
part: the result aggregate records a counted `aggregate ⊇ member` edge, so the
caller's one `DecrefValueRegion` on the aggregate cascades the member region
down to its **birth** reference and stops there. Nothing else names it.

So a helper building a piece of a native's result takes the call's `&Alloc` and
allocates through it. One region carries the whole result, one release reclaims
it, and the `Fresh` declaration the primitive makes ([effects.md](effects.md))
is true of the members as well as the aggregate. Two helpers sit on this seam:
`traitregistry::call_method_fn` (below) and `io::Completion::to_value`, whose
structs `io/wait` / `io/reap` collect into the array they return. The reference
is the test: `tests/elle/region-io-completion-leak.lisp` measures a pumped io
loop bounded, and
`runtime::tests::ownership::region_native_trait_dispatch_fresh_result_reclaims`
pins the trait-dispatch face.

## VM access through the ctx — per-instance VM state

`ctx.vm()` is how a native reaches the driving VM for **state access** (read
`vm.user_args`, `vm.ffi`, the loaded-module set) and for **synchronous execution
re-entry** (`vm.call_closure`, `execute_bytecode_saving_stack` — a trait-method
closure, a module load). It resolves the call's own driving VM, so two embedded
instances on one thread each read their own VM state — pinned by
`two_instances_read_their_own_vm_args` (`src/runtime/tests/lifecycle.rs`).

The one caller with no ctx is the **FFI callback trampoline** (`ffi/callback.rs`),
invoked by C with nothing to thread. It captures its VM explicitly at registration
(`create_callback`, reached from `prim_ffi_callback` which has `ctx.vm()`), storing
a `*mut VM` in `CallbackData` (synthetic re-entry: it captures its VM at
registration, since C threads none).

Three pieces of *per-VM* state are `VM` fields (so two instances never collide),
reached as `ctx.vm().field`:

- **`vm.exit_trapped`**: when set, `(exit)`
  emits a catchable `{:error :exited :code N}` instead of terminating the process.
  One VM per worker thread, so the trap stays scoped to the calling OS thread, as
  the test runner relies on.
- **`vm.ffi` callback-error slot**: the
  trampoline stashes a closure's error here; `prim_ffi_call` drains it via
  `ctx.vm().ffi_mut()`. Single-thread by the documented callback limitation.
- **`vm.active_tier`**: the backend tier a
  forced-tier dispatch (`compile/run-on`) is running under, read by `(vm/tier)` /
  `(backend? :tier)`. Set/restored around the dispatch in
  `dispatch_compile_run_on`.

## Symbols are not a per-instance capability

A symbol id is the name's hash and the hash→name registry is process-global
([impl/symbol.md](../symbol.md)), so name resolution needs no instance, no ctx,
and no threading. `crate::symbol::name(id)` answers anywhere — inside
`Display`/`Debug for Value`, inside a panic message, inside a unit test with no
`Runtime` at all. There is no lost-names case to work around.

The `SymbolTable` a `RuntimeCore` owns is a handle onto that registry, not a
table of its own. It is still threaded through the pipeline (`&mut SymbolTable`
where interning happens, `vm.symbols()` / `ctx.vm().symbols()` where a VM is in
scope), because interning is a compile-time capability — but two instances
cannot disagree about a name, and none of the reach paths above affects what a
symbol means.

Pinned by `two_instances_agree_on_every_symbol_name`
(`src/runtime/tests/lifecycle.rs`): a symbol one `Runtime` compiled is the same
symbol in the other.

## JIT intrinsic helpers reach the VM through a `JitCtx`

The JIT fast-path intrinsics (`%put`/`%del`/`%has?`/`%array-push`/`%string-push`/
`%bytes-push`/`%freeze`/`%thaw`) lower to `extern "C"` helpers (`elle_jit_*`,
`src/jit/runtime/ops.rs`) that run the same `PrimFn` bodies as the interpreter, so
they need a VM-bearing `NativeCtx`. They have no `NativeCtx` to call `ctx.vm()` on —
only raw `(tag, payload)` operands handed up from compiled code — and each must reach
its own instance's VM so two coexisting instances on one thread stay isolated.

The vehicle is **`JitCtx`** (`src/jit/mod.rs`): a `*mut`-threadable capability
bundle carrying the driving VM. The compiled function's prologue
(`src/jit/compiler/translate.rs`) builds one in a stack slot from its `vm` entry
parameter, and the intrinsic emit sites thread its address as the helper's last
argument; the helper resolves the VM from it and builds the `NativeCtx`
(`run_alloc_intrinsic` for the allocating intrinsics, `boundary_vm`/`with_region_vm`
for the others). `run_alloc_intrinsic` (`src/vm/types.rs`) — the one body shared by
the interpreter intrinsic handlers and the JIT helpers — takes the VM explicitly, so
the VM is named on both tiers. `JitCtx` is `#[repr(C)]` with the VM at offset
0, matching the prologue's raw store; the heap axis extends the same bundle with a
heap capability, threaded the same way, so the allocating intrinsics name their heap
too without another ABI change. Pinned by `jit_intrinsics_use_threaded_vm`
(`src/jit/dispatch/tests.rs`): each intrinsic helper runs correctly off the
threaded `JitCtx`.

## The `ctx.*` allocation surface

A native body reads `ctx.string("x")`, `ctx.pair(a, b)`, `ctx.alloc(obj)` — the
region is implicit in the capability. The `ctx.*` constructors are uniform
one-line forwarders (one spec line each), so adding a heap type is a one-liner.

```rust
fn prim_x(ctx: &mut NativeCtx, args: &[Value]) -> (SignalBits, Value) {
    let s = ctx.string(text);
    let p = ctx.pair(head, tail);
    …
}
```

Helpers shared by several primitives take `ctx: &mut NativeCtx` (or a `region:
RuntimeRegion`) as needed; pure helpers that never allocate take nothing.

Rich error values (`{:error :kind :message … :field v}`) are built through the
one region-coherent routine `error_extra` and its `rich_error!` macro sugar — so
the error's message and every field are born in one region. See
[errors.md](errors.md).

## Direct `PrimFn` invocation sites (not via VM dispatch)

These call a primitive's function pointer directly. The host/plugin sites build a
ctx at the flip, minting the per-call region exactly as `dispatch_native_call`
does:

- The WASM hosts: `call_primitive` (full backend) and the tiered linker's
  `rt_call` NativeFn branch (`src/wasm/lazy/env.rs`).
- `plugin_api::call_plugin` — see Plugins.

`traitregistry::call_method_fn` also calls a native's pointer directly, but does
**not** mint: it runs the resolved trait-method native against the *outer* call's
ctx (`prim_fn(ctx, args)`), so a fresh method result — `(rest [array])`'s copied
tail — lands in that call's `alloc_region` and is recognised as fresh by
`dispatch_native_call`. A separate `boundary` region here would strand a
genuinely-fresh result (a region distinct from `alloc_region`), which the
pass-through accounting then mis-reads as a borrow and over-retains — the region
never freed (pinned bounded by
`runtime::tests::ownership::region_native_trait_dispatch_fresh_result_reclaims`).
The closure branch routes through `call_closure`.

The compiler enumerates the rest: a non-conforming caller is a type error.

## The compiler's allocation: `arena::alloc_in_region`

The env builder and literal materialization resolve a `RuntimeRegion` directly
(through the activation's `activation_region_map` / `env_value_region`) and
allocate with `arena::alloc_in_region(obj, region)` — they have no ctx and need
none. `populate_env` mints one fresh physical region per env value, so the env builder
names each value's region directly.

## Plugins

The stable ABI's resolved API constructors (`make_string`, `make_bytes`,
`make_array`, `make_struct`, … — `plugin_api/capi.rs`) allocate into the plugin
call's region. `call_plugin` builds the call's `(region, heap)` capability as a
`CallCtx` and passes it — as an opaque first argument — to the plugin primitive,
which threads it back into every allocating constructor. The capability is thus a
value on the call stack, not ambient state: a plugin cannot allocate without a
region any more than a built-in native can, and because each call carries its own
ctx, re-entry (a plugin API function that itself dispatched a plugin) could not
confuse the region — there is no single ambient slot to clobber. The opaque
`CallCtx` is elle-internal; the plugin never dereferences it. Changing the
primitive's signature to carry the ctx is an ABI break, gated by the loader
`version` (currently 3): a plugin built against a different ABI version fails to
load rather than mismatching the calling convention.

## What becomes unrepresentable

| Defect | Why it cannot be written |
|---|---|
| A native allocates without a region | `PrimFn` hands it a `ctx` and nothing else |
| An allocation whose region is left implicit | every allocator names its region |
| A native allocates into a region its caller already released | the ctx owns a fresh per-call region; nothing hands a native another |
| A save/restore imbalance corrupting a sibling call's region | each call's region is private to its ctx |
| A native reaching a different instance's VM | `ctx.vm()` resolves the call's own driving VM |
| A native resolving a symbol to a different instance's name | a symbol id is the name's hash ([impl/symbol.md](../symbol.md)); there is no per-instance name to get wrong |
| A `vm()` call on a context that has no VM | only `NativeCtx` (always VM-bearing) has `vm()`; an allocation-only site holds an `Alloc`, which has none |
| `exit`-trap / FFI-callback-error / active-tier state leaking between coexisting instances | each is a `VM` field |

## Per-instance compile state

The same explicit-capability rule governs *compile-time* state, so two embedded
Elle instances can compile and run in one process — even on one thread — without
sharing macro definitions, stdlib exports, or REPL bindings. The capability is
**`CompileCtx`** (`src/pipeline/cache.rs`): the macro-expansion VM, the
prelude/core `Expander`, the resident `PrimitiveMeta` (primitives + core.lisp +
stdlib exports + REPL value bindings), and the file→signal projection cache. A
compile names its instance's `CompileCtx` or it does not compile.

An instance's three capabilities — the `VM`, the `SymbolTable`, and the
`CompileCtx` — are owned together by a **`RuntimeCore`** (`src/runtime.rs`), which
hands them out as the disjoint borrows `parts() -> (&mut VM, &mut SymbolTable,
&mut CompileCtx)` that the pipeline entry points
(`compile`/`compile_file`/`eval`/`analyze`/`execute_scheduled`) thread
explicitly. `register_stdlib_exports`, the REPL-binding registration, and the
projection lookup are `CompileCtx` methods. Two owners construct a `RuntimeCore`:
`Runtime` (the `elle foo.lisp` / REPL / embedding path) and the `os/spawn` worker
(`src/primitives/concurrency.rs`), so a spawned thread compiles against its own
instance, never a shared cache.

Three seams reach the compile context where no `CompileCtx` parameter is in
scope, without reintroducing shared state:

- **macro-body compiles** (`eval_syntax`, deep in expansion) read the
  primitive+stdlib meta from an `Rc<PrimitiveMeta>` (`eval_meta`) carried on the
  `Expander`, so the expansion chain needs no `CompileCtx` threading;
- the runtime **`eval`/`import`/`compile/*` instructions** reach the instance's
  `CompileCtx` through a `VM`-held pointer (`VM::set_compile_ctx`, the `heap_ptr`
  idiom), set by the owner;
- the **analyzer's import-projection compile** reaches it through a frontend-set
  pointer (`Analyzer::set_compile_ctx`), so projections are the instance's own.

The pinning counterfactual is `two_instances_interleaved_defs_are_isolated`
(`src/runtime/tests/lifecycle.rs`): two `Runtime`s on one thread each maintain their own
top-level `(def x …)` via `compile_file_repl` +
`CompileCtx::register_repl_binding`, and each reads back only its own binding — a
shared compile cache fails it. (The symbol table and heap are threaded
separately; this isolates the *compile* state, which is what lets one instance's
stdlib/REPL definitions stay invisible to another.)

## Gates

- The declaration oracle ([effects.md](effects.md)) polices result
  regions: a body that allocates its result into the wrong region panics in
  debug, naming the primitive.
- `--trace=guardfree` over the region suite is the UAF oracle for every change to
  the dispatch/ctx path.
