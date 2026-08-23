use super::*;

mod region;

/// LIR instruction (SSA form - each register assigned exactly once)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LirInstr {
    // === Constants ===
    /// Load a constant into a register
    Const { dst: Reg, value: LirConst },
    /// Load a Value constant into a register
    ValueConst {
        dst: Reg,
        value: crate::value::Value,
    },
    /// Materialize a heap literal — a string, or a quoted compound structure
    /// (list/array/nested data) — from a recursive immutable `ConstTemplate`
    /// into a FRESH value in this literal's OWN solver-assigned region.
    ///
    /// Unlike `Const`/`ValueConst` (pure pool loads with no region), this is an
    /// ordinary allocation site, exactly like `List`/`MakeArrayMut`/`MakeClosure`:
    /// the value is born in `region` (resolved per activation), freed at its
    /// `decref_point`, and every escape is tracked by normal RC. The whole
    /// structure shares the one `region` (an immutable aggregate). The `template`
    /// itself stays immutable compile-time data
    /// (docs/impl/region/model.md § "Constants lower as ordinary allocations").
    MaterializeConst {
        dst: Reg,
        template: crate::value::ConstTemplate,
        region: StaticRegion,
    },

    // === Variables ===
    /// Load from local slot
    LoadLocal { dst: Reg, slot: u16 },
    /// Store to local slot
    StoreLocal { slot: u16, src: Reg },
    /// Store to local slot with refcount bookkeeping.
    /// decref_and_free(old), incref(new), then store.
    /// Used for mutable binding init and assignment.
    StoreLocalRefcounted { slot: u16, src: Reg },
    /// Load from capture (auto-unwraps LocalCell)
    LoadCapture { dst: Reg, index: u16 },
    /// Load from capture without unwrapping (for forwarding cells to nested closures)
    LoadCaptureRaw { dst: Reg, index: u16 },
    /// Store to capture (handles cells automatically)
    StoreCapture { index: u16, src: Reg },
    // === Closures ===
    /// Create a closure. The closure body is in the module's closure
    /// list at the given `ClosureId`, not owned by this instruction.
    MakeClosure {
        dst: Reg,
        closure_id: ClosureId,
        captures: Vec<Reg>,
        /// Slot the closure (and its inline-captured env) is allocated into.
        region: StaticRegion,
    },
    /// Push the currently-executing closure — every reference to a lambda's own
    /// self-recursive binding, in **both** value and call position. Value position
    /// (`loop`/`go` passed to a higher-order call, returned, or stored) uses the
    /// closure as a value; call position (`(go …)`) uses it as the callee, so the
    /// call re-enters the same code+env with new args (self-call re-dispatch).
    /// Takes no operand and names no capture slot: the executing closure is a
    /// runtime register the interpreter reads directly (`Fiber::current_closure`,
    /// restored across every call/tail-call/suspend boundary), the JIT receives as
    /// a compiled-body parameter (`self_tag_payload`, which its self-tail-call
    /// optimization also matches against), and the WASM backend reads from a
    /// reserved linear-memory self slot the host installs at each closure entry
    /// (`src/wasm/emit.rs` `SELF_SLOT`). The value it yields is the closure itself,
    /// so invoking it recurses correctly (`src/runtime/tests/selfrec.rs`).
    LoadSelf { dst: Reg },

    // === Function Calls ===
    /// Call a function (callee is known to not suspend).
    /// When `arity_checked` is true, the compiler verified arity at compile time.
    Call {
        dst: Reg,
        func: Reg,
        args: Vec<Reg>,
        arity_checked: bool,
        /// Slot for this call's result / per-call region routing.
        region: StaticRegion,
    },
    /// Call a function that may suspend (yield, I/O, etc.).
    /// The WASM emitter creates a call-site continuation with spill/restore
    /// so the caller can resume if the callee yields.
    /// Only emitted inside functions whose signal includes may_suspend().
    /// When `arity_checked` is true, the compiler verified arity at compile time.
    SuspendingCall {
        dst: Reg,
        func: Reg,
        args: Vec<Reg>,
        arity_checked: bool,
        region: StaticRegion,
    },
    /// Tail call (no return)
    /// When `arity_checked` is true, the compiler verified arity at compile time.
    TailCall {
        /// The call's result register. A native callee that completes normally
        /// does NOT replace the frame (a native pushes no bytecode frame), so
        /// the compiler's own post-`TailCall` owned-arg releases must run before
        /// the trailing `Return` — the Inc4 native-tail trick (src/vm/call.rs
        /// `tail_call_inner`; docs/impl/region/rules.md Rule 8). The JIT binds `dst` to
        /// the native's result on that completion path so it can keep
        /// translating those releases instead of returning at the call. The
        /// stack-based interpreter ignores `dst`: a normally-completing native
        /// leaves its result on the operand stack, which `Return` pops (the
        /// `Return(dst)` reg is untracked and falls through `ensure_on_top`'s
        /// already-on-top case). For a closure callee the frame IS replaced
        /// (the trampoline runs it) and `dst` is unused — the moved args are
        /// released by the owned-param callee, not here.
        dst: Reg,
        func: Reg,
        args: Vec<Reg>,
        arity_checked: bool,
        region: StaticRegion,
        /// True when the callee is a per-call *local* closure whose
        /// compiler-emitted release is dead past this frame-replacing `TailCall`
        /// (`lower_call` sets it from the region solver: the closure's region has
        /// its `decref_point` at this call). The runtime then ADOPTS the closure
        /// — releasing its region when the new activation completes — supplying
        /// the decref the dead trailing block skipped. False for a program-root
        /// callee (a top-level `defn`, no per-call decref) or a native/collection
        /// callee (no frame replacement). See `TailCallInfo` and
        /// `tests/elle/region-tailcall-closure-callee-leak.lisp`.
        defer_callee_release: bool,
        /// The static slot of a **closure-cycle merged arena** this letrec body
        /// tail-calls a NON-member out of — set by `lower_call` from
        /// `RegionInfo::cycle_tail_release` (docs/impl/region/letrec.md § The letrec
        /// closure-cycle merge). The merged arena's binding-scope `DecrefRegion`
        /// is dead past this frame-replacing `TailCall`, so when the callee turns
        /// out to be a **closure** at runtime (a redefined operator, a foreign
        /// fn) the new activation resolves this slot through its region map and
        /// ADOPTS the arena — `deferred_releases` frees it once at the recursion's
        /// completion. When the callee is a **native** (a `NativeFn` — a funnel
        /// op like `%array-push`, or a rebound operator's value-position face) the
        /// frame is NOT replaced, this slot is never consumed, and the live
        /// scope-exit `DecrefRegion` frees the arena instead — the two paths are
        /// mutually exclusive per call, so exactly one release fires however the
        /// callee resolves (the compiler never classifies it). `None` for every
        /// tail call that is not a merged clique's non-member body tail; distinct
        /// from `defer_callee_release` (the MEMBER path, `region_of(callee)`), never both
        /// set on one call.
        deferred_release_slot: Option<StaticRegion>,
        /// The frame locals holding this call's BORROWED arguments — the fresh
        /// owning reference the frame mints per borrowed argument so the callee
        /// has one to release (rules.md Rule 5, move-on-tail-call), stashed
        /// where the post-`TailCall` block can name it.
        ///
        /// That retain has exactly one consumer per path: a frame-replacing
        /// CLOSURE callee's owned-param release, or — the frame not being
        /// replaced by a native — the fall-through block's own
        /// `DecrefValueRegion`. A native that leaves by a SIGNAL reaches
        /// neither, so the runtime consumes it there instead
        /// (docs/impl/region/mechanism.md § "What the fall-through owes, a
        /// signal exit owes too"). Empty for a call with no borrowed argument,
        /// which is the overwhelming majority.
        borrowed_arg_slots: Vec<u16>,
    },

    // === Data Construction ===
    /// Construct a cons cell
    List {
        dst: Reg,
        head: Reg,
        tail: Reg,
        region: StaticRegion,
    },
    /// Construct an array
    MakeArrayMut {
        dst: Reg,
        elements: Vec<Reg>,
        region: StaticRegion,
    },
    /// Get car of cons
    First { dst: Reg, pair: Reg },
    /// Get cdr of cons
    Rest { dst: Reg, pair: Reg },

    // === Primitive Operations ===
    /// Binary arithmetic
    BinOp {
        dst: Reg,
        op: BinOp,
        lhs: Reg,
        rhs: Reg,
    },
    /// Unary operations
    UnaryOp { dst: Reg, op: UnaryOp, src: Reg },
    /// Type conversion (float↔int intrinsics)
    Convert { dst: Reg, op: ConvOp, src: Reg },
    /// Comparison
    Compare {
        dst: Reg,
        op: CmpOp,
        lhs: Reg,
        rhs: Reg,
    },

    // === Type Checks ===
    /// Check if value is nil
    IsNil { dst: Reg, src: Reg },
    /// Check if value is a pair
    IsPair { dst: Reg, src: Reg },
    /// Check if value is an array (for pattern matching)
    IsArray { dst: Reg, src: Reg },
    /// Check if value is an @array (for pattern matching)
    IsArrayMut { dst: Reg, src: Reg },
    /// Check if value is a struct (for pattern matching)
    IsStruct { dst: Reg, src: Reg },
    /// Check if value is an @struct (for pattern matching)
    IsStructMut { dst: Reg, src: Reg },
    /// Check if value is an immutable set (for pattern matching)
    IsSet { dst: Reg, src: Reg },
    /// Check if value is a mutable set (for pattern matching)
    IsSetMut { dst: Reg, src: Reg },
    /// Get array length (for pattern matching)
    ArrayMutLen { dst: Reg, src: Reg },

    // === Capture Cell Operations (for mutable captures) ===
    /// Create a capture cell containing a value
    MakeCaptureCell {
        dst: Reg,
        value: Reg,
        region: StaticRegion,
    },
    /// Load value from capture cell
    LoadCaptureCell { dst: Reg, cell: Reg },
    /// Store value into capture cell
    StoreCaptureCell { cell: Reg, value: Reg },

    // === Destructuring ===
    /// No match arm covered the scrutinee: signals :match-error carrying it
    MatchFail { dst: Reg, src: Reg },
    /// First for destructuring: signals error if not a cons cell
    FirstDestructure { dst: Reg, src: Reg },
    /// Rest for destructuring: signals error if not a cons cell
    RestDestructure { dst: Reg, src: Reg },
    /// Array ref for destructuring: signals error if out of bounds or not an array
    ArrayMutRefDestructure { dst: Reg, src: Reg, index: u16 },
    /// Array slice from index: returns a new array from index to end, or empty array
    ArrayMutSliceFrom { dst: Reg, src: Reg, index: u16 },
    /// Table/struct get with silent nil: nil if key missing/wrong type. Used by match.
    /// `key` is a constant pool index holding a keyword Value.
    StructGetOrNil { dst: Reg, src: Reg, key: LirConst },
    /// Table/struct get for destructuring: signals error if key missing or wrong type.
    /// `key` is a constant pool index holding a keyword Value.
    StructGetDestructure { dst: Reg, src: Reg, key: LirConst },

    /// Struct rest for destructuring: collect all keys from src NOT in exclude_keys
    /// into a new immutable struct. Used by `{:a a & rest}` patterns.
    /// `exclude_keys` are constant pool entries (keywords or symbols).
    StructRest {
        dst: Reg,
        src: Reg,
        exclude_keys: Vec<LirConst>,
    },

    // === Silent destructuring (parameter context: absent optional params → nil) ===
    /// First with silent nil: returns nil if not a cons cell.
    /// Used for &opt/(required) parameter destructuring where absent values produce nil.
    FirstOrNil { dst: Reg, src: Reg },
    /// Rest with silent empty-list: returns EMPTY_LIST if not a cons cell.
    /// Used for &opt/(required) parameter destructuring.
    RestOrNil { dst: Reg, src: Reg },
    /// Array ref with silent nil: returns nil if out of bounds or not an array.
    /// Used for `&opt`/\[required\] parameter destructuring.
    ArrayMutRefOrNil { dst: Reg, src: Reg, index: u16 },

    // === Fibers ===
    /// Load the resume value after a yield.
    /// This is the first instruction in a yield's resume block.
    /// At runtime, the resume value is on top of the operand stack
    /// (pushed by the VM's resume_continuation).
    LoadResumeValue { dst: Reg },

    // === Runtime Eval ===
    /// Runtime eval: compile and execute a datum.
    /// Pops env and expr from stack, compiles and executes, pushes result.
    Eval { dst: Reg, expr: Reg, env: Reg },

    // === Splice Support ===
    /// Extend an array with all elements of an indexed type (array or @array).
    /// Used by splice path: builds the args array incrementally.
    ArrayMutExtend { dst: Reg, array: Reg, source: Reg },
    /// Append a single value to an array.
    /// Used by splice path: adds non-spliced args to the args array.
    ArrayMutPush { dst: Reg, array: Reg, value: Reg },
    /// Call a function with elements of an array as arguments.
    /// The array is unpacked into individual arguments at runtime.
    CallArrayMut {
        dst: Reg,
        func: Reg,
        args: Reg,
        region: StaticRegion,
    },
    /// Tail call with elements of an array as arguments.
    TailCallArrayMut {
        func: Reg,
        args: Reg,
        region: StaticRegion,
    },

    // === Allocation Regions ===
    /// Increment the reference count of a region named by a **static slot**.
    /// Emitted when a value in one region is stored into a structure
    /// in another region (cross-region reference).
    ///
    /// `region_id` is a `StaticRegion` — a *compile-time slot*, not a physical
    /// region. The handler resolves it through the current activation's
    /// `activation_region_map` to the physical `RuntimeRegion` this execution
    /// minted for that slot. This is the **slot-resolved** half of the region-RC
    /// split, the counterpart to the value-resolved `IncrefValueRegion`/
    /// `DecrefValueRegion`: a slot is usable only when the lowerer can name the
    /// region statically (the allocation is local to this function), which is
    /// exactly the precondition compile-time region coalescing harvests
    /// (docs/impl/region/mechanism.md § "Compile-time region selection (coalescing)").
    /// It touches no operand stack.
    IncrefRegion { region_id: StaticRegion },

    /// Decrement the reference count of a region.
    /// Emitted by the lowerer at each region's `decref_point` HirId
    /// (the value's last use). Decrements RC; when RC hits 0, the
    /// region's pages are freed and cascade decrefs fire for any
    /// cross-region references found in the region's contents.
    /// The sole region-demise LIR instruction.
    DecrefRegion { region_id: StaticRegion },

    /// Decrement the reference count of the *runtime* region of the value
    /// in `src`. The handler reads the value, calls `result_region_of` to
    /// find the runtime region, and decrefs it unconditionally (immediates,
    /// which have no region, excepted). This is the caller half of the
    /// prediction-free return convention: the callee handed back one owning
    /// reference via `IncrefValueRegion`, and this consumes it at the result
    /// binding's decref_point. The target is always the value's runtime
    /// region — never a static slot — so there is no `expected` gate.
    DecrefValueRegion { src: Reg },

    /// Decrement the reference count of the *runtime* region of the value in
    /// `src`, using `region_of` (NOT `result_region_of`). Unlike
    /// `DecrefValueRegion`, this does NOT see through a `CaptureCell` wrapper —
    /// it frees the CELL's own region. Emitted at a captured (env-allocated)
    /// binding's `decref_point` to release the per-value env cell
    /// `populate_env` minted for it (docs/impl/region/rules.md Rule 8). Using
    /// `DecrefValueRegion` here would free the inner value's (caller-owned)
    /// region and leak the cell.
    DecrefCellRegion { src: Reg },

    /// Increment the reference count of the region of the value in
    /// `src`. The handler reads the value, calls `region_of` to find
    /// the runtime region, and increfs it (skipping region 0, an
    /// immediate, which never participates in RC). The mirror of
    /// `DecrefValueRegion`, but unconditional (no `expected` gate):
    /// it is emitted by the return-wrapping pass at every function's
    /// tail value so the callee hands the caller exactly one owning
    /// reference to the result's *runtime* region — whatever it turns
    /// out to be (a fresh callee allocation, a passed-through arg, or a
    /// branch-dependent mix). The caller balances it with a
    /// `DecrefValueRegion` at the result binding's decref_point. This is
    /// the prediction-free calling convention: the result region is
    /// never named at compile time, only read from the value at
    /// runtime.
    ///
    /// **`src` (value-resolved) vs `region_id` (slot-resolved).** This
    /// instruction carries a register `src` and reads the region *from the
    /// value at runtime* — the honest encoding whenever the region cannot be
    /// named at compile time. `IncrefRegion`/`DecrefRegion` instead carry a
    /// static `region_id` slot and resolve it through the activation map. When
    /// the lowerer can prove the tail value is a fresh local allocation whose
    /// region is a known slot, it substitutes the slot-resolved `IncrefRegion`
    /// for this value-resolved mint (one fewer runtime deref, stack-neutral);
    /// the substitution is guarded by `AssertRegionMatches`
    /// (docs/impl/region/mechanism.md § "Compile-time region selection (coalescing)").
    IncrefValueRegion { src: Reg },

    /// Link the region of `child` as an **Owned** member of the region of
    /// `parent`'s subtree — the runtime `AdoptRegion` of the ownership forest
    /// (docs/impl/region/ownership.md § "Adoption and subtree drop"). Value-resolved,
    /// like `IncrefValueRegion`/`DecrefValueRegion`: the handler reads both
    /// values, resolves their *runtime* regions (`result_region_of`), and calls
    /// `RegionStore::adopt_region`, which freezes the child's RC. No operand
    /// region slot — the regions an Owned subtree links are runtime facts
    /// (call-results / cross-activation allocations that can never be a static
    /// slot; a tight static case MERGEs instead, § Merging). The handler pops
    /// both values (loaded from their binding slots by the lowerer purely to
    /// drive the adopt).
    ///
    /// Emitted by the ownership forest. Realized on the interpreter
    /// (`handle_adopt_region`) and the JIT (`elle_jit_adopt_region`, a
    /// line-for-line mirror), so the same program adopts identically on either
    /// tier; on WASM the op is a structural no-op and on MLIR a region-op-carrying
    /// function is GPU-ineligible, so prompt reclamation on those tiers awaits their
    /// structural-arena handling (step 5).
    AdoptRegion { parent: Reg, child: Reg },

    /// Link the region of `child` as an **Owned** member of the region of
    /// `parent`'s subtree, resolving BOTH operands with `region_of` — NOT
    /// `result_region_of`. The one difference from [`AdoptRegion`](Self::AdoptRegion):
    /// it does **not** see through a `CaptureCell` wrapper, so it adopts the CELL's
    /// **own** region (the capture-cell↔closure containment the ownership forest
    /// needs to reclaim a local recursive/letrec closure clique as a unit —
    /// docs/impl/region/adopt.md § "The capture adopt"). `AdoptRegion` would unwrap a
    /// cell operand to its content (a self-edge no-op for `cell ⊇ content`, and the
    /// content — skipping the cell — for `closure ⊇ cell`), so a cell's own region is
    /// otherwise unreachable by any ownership cut (only `DecrefCellRegion` names it).
    /// This is the `region_of`-adopt counterpart, exactly as `DecrefCellRegion` is the
    /// `region_of` counterpart of `DecrefValueRegion`.
    ///
    /// Value-resolved, no region slot: the handler pops both values (loaded from
    /// their binding slots purely to drive the adopt), resolves their runtime regions
    /// with `region_of`, and calls `RegionStore::adopt_region`, freezing the child's
    /// RC. Realized on the interpreter (`handle_adopt_cell_region`) and the JIT
    /// (`elle_jit_adopt_cell_region`); a structural no-op on WASM and GPU-ineligible
    /// on MLIR, like `AdoptRegion`.
    AdoptCellRegion { parent: Reg, child: Reg },

    /// Free a **co-owned region group** as one unit — the runtime `FreeRegionGroup`
    /// of the ownership forest. An
    /// externally-unique mutual reference cycle with no container parent has no owner
    /// among its members (each owns and is owned by the others), so it is reclaimed
    /// symmetrically: every member's runtime region is freed together at the group's
    /// collective last use (the latest member `decref_point`), in place of the members'
    /// individual decrefs. Value-resolved like `AdoptRegion`: the handler pops each
    /// `members` value (the lowerer loads them from their binding slots purely to drive
    /// the free), resolves each to its runtime region, and calls
    /// `FiberHeap::free_region_group`, which runs the four-phase subtree drop over the
    /// whole set so interior member↔member references reclaim with the group and only
    /// genuinely-Shared frontier references cascade. The drop is wholesale, independent
    /// of the members' reference counts.
    ///
    /// Emitted by the ownership forest. Realized on the interpreter
    /// (`handle_free_region_group`) and the JIT (`elle_jit_free_region_group`),
    /// like `AdoptRegion`; a structural no-op on WASM and GPU-ineligible on MLIR
    /// (step 5).
    FreeRegionGroup { members: Vec<Reg> },

    /// Adopt the region of `child` as an **Owned** member of the CURRENT
    /// ACTIVATION's owner node — the pages-less forest root realizing
    /// owner = activation (docs/impl/region/owner.md § "Owner nodes — an
    /// activation as a forest root"). Value-resolved like `AdoptRegion`
    /// (`result_region_of` unwraps a capture cell) but carrying NO parent
    /// operand and NO static slot: the parent is the executing activation's
    /// node, minted lazily at the first adopt and freed implicitly at the
    /// activation's normal completion (the interpreter trampoline's clean
    /// break; the compiled `Return` path) — never by an emitted drop. The
    /// handler pops the child value (loaded purely to drive the adopt); an
    /// immediate child (no region) adopts nothing and mints no node.
    ///
    /// Realized on the interpreter (`handle_adopt_into_activation`) and the
    /// JIT (`elle_jit_adopt_into_activation`); handled structurally (no-op)
    /// on WASM; GPU-ineligible. Emitted by the ownership forest for the
    /// capture-back-edge SCC (a container captured by a closure it holds —
    /// `RegionInfo::activation_adopt_sites`, one adopt per member at the SCC's
    /// enclosing-scope site) and for the transferred returned subtree (a
    /// summarized producer's consumer sites — `RegionInfo::transfer_adopt_regions`,
    /// where this replaces the result's `DecrefValueRegion`). The handlers are
    /// idempotent on an already-Owned child (a re-delivered region keeps its
    /// first owner — docs/impl/region/owner.md § "Owner nodes").
    AdoptIntoActivation { child: Reg },

    /// Debug-only equivalence oracle for compile-time region coalescing: assert
    /// that a static region **slot** resolves to the same physical region the
    /// value **actually** lives in. No-op everywhere except the bytecode
    /// interpreter.
    ///
    /// Emitted immediately before a coalesced `IncrefRegion { region_id }` (the
    /// slot-resolved substitution for a value-resolved `IncrefValueRegion { src }`):
    /// `region_id` is the slot the coalescer chose, `src` is the value whose
    /// region that slot is claimed to name. The interpreter handler peeks `src`
    /// (it must stay on the stack as the return value — never pops), resolves
    /// `region_id` through the current `activation_region_map`, and panics if
    /// `activation_region_map.resolve(region_id) != region_of(src)`. A
    /// mis-coalesce — a slot resolving to the wrong physical region — is a UAF
    /// in waiting (its cascade would free a live region); this turns it into a
    /// deterministic panic at the exact instruction, under the trustworthy
    /// guardfree oracle, instead of a later heap corruption (the mirror of the
    /// native-effect declaration oracle, docs/impl/region/effects.md).
    ///
    /// Build/tier behavior: under `debug_assertions` the interpreter performs
    /// the check; in release it reads the slot operand and does nothing (the
    /// lowerer emits this LIR instruction only under `debug_assertions`, so
    /// release bytecode never contains it). The JIT and WASM tiers translate it
    /// to nothing; the GPU (MLIR/SPIR-V) tiers exclude any function carrying it
    /// via the `is_gpu_instruction` whitelist. JIT/WASM coalesced sites are
    /// instead covered by the runner's cross-tier divergence detection and the
    /// escape golden. It renders into no `[region_instrs]` golden line — it is
    /// scaffolding, not part of the semantic RC stream.
    AssertRegionMatches { region_id: StaticRegion, src: Reg },

    // === Dynamic Parameters ===
    /// Push a parameter frame. `pairs` contains (param_reg, value_reg) pairs.
    /// All param/value registers are consumed from the stack.
    PushParamFrame { pairs: Vec<(Reg, Reg)> },
    /// Pop the top parameter frame.
    /// No registers produced or consumed.
    PopParamFrame,

    // === Signal Checking ===
    /// Check that a closure's signal satisfies a bound.
    /// If the value in `src` is a closure whose `signal.bits & !allowed_bits != 0`,
    /// signal `:error`. Non-closures pass silently.
    /// If the check passes, execution continues.
    CheckSignalBound {
        src: Reg,
        allowed_bits: crate::value::fiber::SignalBits,
    },

    // === Type predicates (intrinsics) ===
    /// Check if value is the empty list
    IsEmpty { dst: Reg, src: Reg },
    /// Check if value is a boolean
    IsBool { dst: Reg, src: Reg },
    /// Check if value is an integer
    IsInt { dst: Reg, src: Reg },
    /// Check if value is a float
    IsFloat { dst: Reg, src: Reg },
    /// Check if value is a string (immutable or mutable)
    IsString { dst: Reg, src: Reg },
    /// Check if value is a keyword
    IsKeyword { dst: Reg, src: Reg },
    /// Check if value is a symbol
    IsSymbolCheck { dst: Reg, src: Reg },
    /// Check if value is bytes (immutable or mutable)
    IsBytes { dst: Reg, src: Reg },
    /// Check if value is a box (lbox)
    IsBox { dst: Reg, src: Reg },
    /// Check if value is a closure
    IsClosure { dst: Reg, src: Reg },
    /// Check if value is a fiber
    IsFiber { dst: Reg, src: Reg },
    /// Get type keyword for a value
    TypeOf { dst: Reg, src: Reg },

    // === Data access (intrinsics) ===
    /// Polymorphic length
    Length { dst: Reg, src: Reg },
    /// Polymorphic get (2 args: collection, key)
    Get { dst: Reg, obj: Reg, key: Reg },
    /// Polymorphic put (3 args: collection, key, value)
    Put {
        dst: Reg,
        obj: Reg,
        key: Reg,
        val: Reg,
    },
    /// Polymorphic del (2 args: collection, key)
    Del { dst: Reg, obj: Reg, key: Reg },
    /// Polymorphic has? (2 args: collection, key)
    Has { dst: Reg, obj: Reg, key: Reg },
    /// Polymorphic push (2 args: collection, value). Mutates @array in place;
    /// returns new array for immutable. Distinct from `ArrayMutPush` (splice).
    IntrPush { dst: Reg, array: Reg, value: Reg },
    /// Append string to @string (2 args: string, value)
    IntrStringPush { dst: Reg, string: Reg, value: Reg },
    /// Append byte to @bytes (2 args: bytes, value)
    IntrBytesPush { dst: Reg, bytes: Reg, value: Reg },
    /// @array pop (1 arg, returns popped value)
    Pop { dst: Reg, src: Reg },

    // === Mutability (intrinsics) ===
    /// Mutable → immutable copy
    Freeze {
        dst: Reg,
        src: Reg,
        region: StaticRegion,
    },
    /// Immutable → mutable copy
    Thaw {
        dst: Reg,
        src: Reg,
        region: StaticRegion,
    },

    // === Identity (intrinsics) ===
    /// Bitwise tag+payload equality
    Identical { dst: Reg, lhs: Reg, rhs: Reg },
}
