// audited: 2026-09-29
//! The bytecode opcode set, and the checked decode from an opcode byte.
//!
//! docs/impl/bytecode.md

/// Bytecode instruction set
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Instruction {
    /// Load constant from constant pool
    LoadConst,

    /// Load local variable (index u16)
    LoadLocal,

    /// Store local variable (index u16)
    StoreLocal,

    /// Load from closure environment
    LoadUpvalue,

    /// Load from closure environment WITHOUT unwrapping cells (for capture forwarding)
    LoadUpvalueRaw,

    /// Store to closure environment
    StoreUpvalue,

    /// Pop value from stack
    Pop,

    /// Duplicate top of stack
    Dup,

    /// Duplicate value at offset from top of stack (offset u8)
    /// offset 0 = top, offset 1 = second from top, etc.
    DupN,

    /// Function call. Operands: u16 arg count, u32 result region slot.
    Call,

    /// Tail call. Operands: u16 arg count, u32 result region slot, u8
    /// defer-callee-release flag, u32 deferred-release slot (`0` for none), then
    /// a u8 count of borrowed-argument slots and one u16 slot each.
    TailCall,

    /// Return from function
    Return,

    /// Jump unconditionally (offset i32)
    Jump,

    /// Jump if false (offset i32)
    JumpIfFalse,

    /// Jump if true (offset i32)
    JumpIfTrue,

    /// Create closure. Operands: u32 region slot, u16 const_idx, u16 num_captures.
    MakeClosure,

    /// Pair cell construction. Operand: u32 region slot.
    Pair,

    /// First operation
    First,

    /// Rest operation
    Rest,

    /// @array construction. Operands: u32 region slot, u8 element count.
    MakeArrayMut,

    /// @array ref. No operands. Pops the index, pops the @array, pushes the
    /// element.
    ArrayMutRef,

    /// @array set. No operands. Pops the value, the index and the @array,
    /// checks their types, and pushes the value. It stores nothing.
    ArrayMutSet,

    /// Integer-only arithmetic (docs/impl/bytecode.md)
    AddInt,
    SubInt,
    MulInt,
    DivInt,

    /// Polymorphic arithmetic (int or float operands)
    Add,
    Sub,
    Mul,
    Div,
    Rem,

    /// Bitwise operations
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    Shl,
    Shr,

    /// Comparisons
    Eq,
    Lt,
    Gt,
    Le,
    Ge,

    /// Type checks
    IsNil,
    IsEmptyList,
    IsPair,
    IsNumber,
    IsSymbol,

    /// Not operation
    Not,

    /// Nil constant
    Nil,

    /// Boolean constants
    True,
    False,

    /// Wrap a value in a capture cell for shared mutable access. Operands: u32
    /// region slot, u8 mutated flag, u64 name symbol. Pops the value, wraps it
    /// in a capture cell, and pushes the cell.
    MakeCapture,

    /// Unwrap a capture cell to get its value
    UnwrapCapture,

    /// Update a capture cell's value
    UpdateCapture,

    /// Emit a signal (suspends execution). Operand: signal bits
    /// (docs/impl/bytecode.md).
    /// `(emit :yield val)` emits SIG_YIELD; `(emit :io val)` emits SIG_IO.
    Emit,

    /// Empty list constant
    EmptyList,

    /// No match arm covered the scrutinee: signals :match-error carrying it.
    MatchFail,

    /// First for destructuring: signals error if not a cons cell.
    FirstDestructure,

    /// Rest for destructuring: signals error if not a cons cell.
    RestDestructure,

    /// Array ref for destructuring: signals error if not an array or out of bounds.
    /// Operand: u16 index (immediate)
    ArrayMutRefDestructure,
    /// Array slice from index (for & rest destructuring): returns sub-array from index to end
    /// Operand: u16 index (immediate)
    ArrayMutSliceFrom,

    /// Type check: is value an array (immutable)?
    IsArray,
    /// Type check: is value an @array (mutable)?
    IsArrayMut,
    /// Type check: is value a struct?
    IsStruct,
    /// Type check: is value a @struct?
    IsStructMut,
    /// Get array length as integer
    ArrayMutLen,
    /// Table/struct get with silent nil (for destructuring): returns nil if key missing or wrong type.
    /// Operand: u16 constant pool index (keyword key)
    StructGetOrNil,

    /// Table/struct get for destructuring: signals error if key missing or wrong type.
    /// Operand: u16 constant pool index (keyword key)
    StructGetDestructure,

    /// First with silent nil (for parameter destructuring): returns nil if not a cons cell.
    /// Used by &opt/(required) parameter destructuring where absent values → nil.
    FirstOrNil,
    /// Rest with silent empty-list (for parameter destructuring): returns EMPTY_LIST if not a pair.
    /// Used by &opt/(required) parameter destructuring.
    RestOrNil,
    /// Array ref with silent nil (for parameter destructuring): returns nil if out of bounds.
    /// Operand: u16 index (immediate)
    ArrayMutRefOrNil,

    /// Runtime eval: pop expr and env from stack, compile+execute, push result.
    Eval,

    /// Extend array with elements of another indexed type (for splice).
    /// Pops source, pops array, pushes extended array.
    ArrayMutExtend,
    /// Push a single value onto an array (for splice).
    /// Pops value, pops array, pushes array with value appended.
    ArrayMutPush,
    /// Call function with elements of an array as arguments (for splice).
    /// Operands: u32 result region slot, u32 args region slot.
    /// Pops args array, pops function, calls function with array elements.
    CallArrayMut,
    /// Tail call with elements of an array as arguments (for splice).
    /// Operands: u32 result region slot, u32 args region slot.
    /// Pops args array, pops function, tail calls with array elements.
    TailCallArrayMut,

    /// Push a parameter frame onto the fiber's param_frames stack.
    /// Operand: u8 count (number of (param, value) pairs on the stack).
    /// Stack: [param1, val1, param2, val2, ...] → [] (all consumed).
    /// Validates each param is a Parameter; signals error if not.
    PushParamFrame,

    /// Pop the top parameter frame from the fiber's param_frames stack.
    /// No operands, no stack effect.
    PopParamFrame,

    /// Type check: is value an immutable set?
    IsSet,
    /// Type check: is value a mutable set?
    IsSetMut,

    /// Check that a closure's signal satisfies a bound.
    /// Operand: `allowed_bits`, signal bits (docs/impl/bytecode.md).
    /// Pops the value from the stack. If it's a closure whose
    /// `signal.bits & !allowed_bits != 0`, signals `:error`.
    /// Non-closures pass silently.
    CheckSignalBound,

    /// Struct rest for destructuring: collect all keys from src NOT in excluded keys.
    /// Operands: u16 count, then count x u16 const_idx (each is a keyword key).
    /// Source struct is popped from the stack; result pushed.
    StructRest,

    /// Convert int → float. Pops value, pushes float. Identity on floats.
    IntToFloat,
    /// Convert float → int (truncation). Pops value, pushes int. Identity on ints.
    FloatToInt,

    // === Intrinsic opcodes ===
    /// Not-equal comparison
    Ne,
    /// Bitwise complement
    BitNotIntr,
    /// Type check: is value a boolean?
    IsBool,
    /// Type check: is value an integer?
    IsInt,
    /// Type check: is value a float?
    IsFloat,
    /// Type check: is value a string (immutable or mutable)?
    IsString,
    /// Type check: is value a keyword?
    IsKeyword,
    /// Type check: is value bytes (immutable or mutable)?
    IsBytes,
    /// Type check: is value a box?
    IsBox,
    /// Type check: is value a closure?
    IsClosure,
    /// Type check: is value a fiber?
    IsFiber,
    /// Get type keyword for a value
    TypeOf,
    /// Polymorphic length
    Length,
    /// Polymorphic get (pops key, pops collection, pushes result)
    IntrGet,
    /// Polymorphic put (pops value, pops key, pops collection, pushes result)
    IntrPut,
    /// Polymorphic del (pops key, pops collection, pushes result)
    IntrDel,
    /// Polymorphic has? (pops key, pops collection, pushes bool)
    IntrHas,
    /// Polymorphic push (pops value, pops collection, pushes result)
    IntrPush,
    /// @array pop (pops @array, pushes popped value)
    IntrPop,
    /// Mutable → immutable copy. Operand: u32 region slot.
    IntrFreeze,
    /// Immutable → mutable copy. Operand: u32 region slot.
    IntrThaw,
    /// Bitwise tag+payload equality (pops b, pops a, pushes bool)
    Identical,

    /// Arity-checked function call, with the operands of `Call`. Compiler
    /// verified arity.
    CallChecked,
    /// Arity-checked tail call, with the operands of `TailCall`. Compiler
    /// verified arity.
    TailCallChecked,

    /// Append string to @string (pops value, pops string, pushes string)
    IntrStringPush,
    /// Append byte to @bytes (pops value, pops bytes, pushes bytes)
    IntrBytesPush,

    /// Increment the reference count of a region named by its slot.
    /// Operand: u32 region slot, resolved through the current activation's
    /// region map; an unmapped slot is skipped. Emitted where the lowerer names
    /// the retained region by its static slot: a store of a value in region A
    /// into a structure in region B, a cross-region capture, a coalesced retain.
    IncrefRegion,

    /// Decrement the reference count of a region named by its slot.
    /// Operand: u32 region slot, resolved and cleared through the current
    /// activation's region map; an unmapped slot is skipped.
    /// Decrements RC; when RC hits 0, the region's pages are freed and
    /// cascade decrefs fire for any cross-region references found in
    /// the region's contents.
    DecrefRegion,

    /// Decrement the reference count of the region of the value on
    /// top of the operand stack. No operand. Pops the value and
    /// calls `result_region_of` + `decref_region` at runtime; an immediate is
    /// skipped. The lowerer emits it where it releases a region by the value
    /// rather than by the slot, such as a call result's decref_point.
    DecrefValueRegion,

    /// Decrement the reference count of the region of the value on top of
    /// the operand stack, using `region_of` (NOT `result_region_of`). No
    /// operand. Pops the value. Unlike `DecrefValueRegion`, this does NOT
    /// see through a `CaptureCell` wrapper — it frees the CELL's own region.
    /// Emitted at a captured (env-allocated) binding's `decref_point` to
    /// release the per-value env cell `populate_env` minted for it (the
    /// owned-binding release for capture cells; docs/impl/region/rules.md).
    /// `DecrefValueRegion` would unwrap to the inner value's region instead —
    /// freeing a caller-owned region and leaking the cell.
    DecrefCellRegion,

    /// Increment the reference count of the region of the value on
    /// top of the operand stack. No operand. Peeks the value, leaving it on the
    /// stack, and increments the region `result_region_of` names; an immediate
    /// is skipped. The mirror of `DecrefValueRegion`. The lowerer emits it at a
    /// function's tail value, so the callee hands the caller one owning
    /// reference to the result's runtime region, and at any retain it names by
    /// the value rather than by the slot.
    IncrefValueRegion,

    /// Adopt the region of one value into another's Owned subtree (the
    /// `AdoptRegion` LIR instruction). No operand. Pops two values — `child`
    /// (top) then `parent` — resolves each to its runtime region via
    /// `result_region_of`, and calls `RegionStore::adopt_region(parent_region,
    /// child_region)`, freezing the child's RC so it is reclaimed only by the
    /// parent's subtree drop (docs/impl/region/ownership.md). A joined region
    /// stays `Counted` (docs/impl/region/colocation.md). Emitted by the
    /// ownership forest; realized on the
    /// interpreter and the JIT (`elle_jit_adopt_region`).
    AdoptRegion,

    /// Adopt the region of one value into another's Owned subtree, resolving
    /// BOTH operands with `region_of` — NOT `result_region_of` (the
    /// `AdoptCellRegion` LIR instruction). No operand. Pops `child` (top) then
    /// `parent`, resolves each to its runtime region via `region_of` (so a
    /// `CaptureCell` operand is NOT unwrapped — its OWN region is used), and calls
    /// `RegionStore::adopt_region`. This is the only ownership cut that can name a
    /// capture cell's own region, letting the forest reclaim a cell↔closure clique
    /// as a unit (docs/impl/region/adopt.md). Emitted by the
    /// ownership forest; realized on the interpreter (`handle_adopt_cell_region`)
    /// and the JIT (`elle_jit_adopt_cell_region`).
    AdoptCellRegion,

    /// Debug-only region-coalescing oracle (the `AssertRegionMatches` LIR
    /// instruction). Operand: u32 region slot. Peeks the value on top of the
    /// operand stack (does NOT pop — the next instruction reads it), resolves the
    /// slot through the current activation's region map, and under `debug_assertions`
    /// panics if the slot's physical region differs from `region_of(value)`. In
    /// release builds it reads the slot operand and does nothing; the lowerer
    /// emits it only under `debug_assertions`, so release bytecode never carries
    /// it. See `LirInstr::AssertRegionMatches`.
    AssertRegionMatches,

    /// Free a co-owned region group as one unit (the `FreeRegionGroup` LIR
    /// instruction). Operand: u8 member count. Pops that many values off the
    /// operand stack, resolves each to its runtime region via `result_region_of`,
    /// and calls `FiberHeap::free_region_group`, which runs the four-phase subtree
    /// drop over the whole set — interior member↔member references reclaim with the
    /// group, only genuinely-Shared frontier references cascade. Emitted by the
    /// ownership forest; realized on the interpreter and the JIT
    /// (`elle_jit_free_region_group`).
    FreeRegionGroup,

    /// Push the currently-executing closure onto the operand stack. No operand.
    /// The value path for a self-reference: the runtime holds the executing
    /// closure in a per-activation register (`Fiber::current_closure`), and this
    /// reads it directly — a value-position `loop`/`go` resolves to the closure
    /// itself with no capture-slot operand. See `LirInstr::LoadSelf`.
    LoadSelf,

    /// Adopt the region of the value on top of the operand stack into the
    /// CURRENT ACTIVATION's owner node (the `AdoptIntoActivation` LIR
    /// instruction). No operand. Pops the child value, resolves its runtime
    /// region via `result_region_of`, lazily mints the activation's pages-less
    /// owner node, and calls `RegionStore::adopt_region(node, child_region)` —
    /// freezing the child's RC so it is reclaimed only by the node's subtree
    /// drop at the activation's normal completion (docs/impl/region/owner.md).
    /// A child region that is already `Owned` keeps its owner. An immediate child
    /// (no region) adopts nothing and mints no node. Realized on the
    /// interpreter and the JIT (`elle_jit_adopt_into_activation`).
    AdoptIntoActivation,

    /// Record a pending join for the next value mint of a region slot
    /// (docs/impl/region/colocation.md). Operand: u32 region slot. Pops the
    /// partner value and records its runtime region and generation on the fiber;
    /// an immediate partner records nothing. The next allocation-slot or
    /// call-slot mint takes the record. A mint of that slot joins the partner's
    /// region when the region is still live and `Counted`, and a mint of another
    /// slot drops the record. Realized on the interpreter and the JIT
    /// (`elle_jit_join_region`).
    JoinRegion,

    /// Materialize a heap literal — a string, or quoted compound data (list /
    /// array / nested structure) — into the region its slot resolves to. Operands:
    /// u32 region slot, u32 template byte length, then that many bytes encoding a
    /// recursive `ConstTemplate` inline in the instruction stream (the immutable
    /// template — plain data kept in the reclaimable bytecode). Resolves the slot
    /// to a physical region and materializes a fresh
    /// structure there, pushing it. Must remain the last variant — the
    /// `Instruction::from_byte` high-water-mark check keys on it.
    MaterializeConst,
}

impl Instruction {
    /// Decode an opcode byte, rejecting bytes that are not a valid
    /// `Instruction`. The ONLY sound way to turn a `u8` into an
    /// `Instruction`: a bare transmute of an out-of-range byte is undefined
    /// behavior (debug builds abort the process, release builds are UB).
    ///
    /// `#[repr(u8)]` with no explicit discriminants assigns variants 0..=N
    /// sequentially; the last variant in source order is the current
    /// high-water mark.
    #[inline]
    pub fn from_byte(byte: u8) -> Option<Instruction> {
        if byte <= Instruction::MaterializeConst as u8 {
            Some(unsafe { std::mem::transmute::<u8, Instruction>(byte) })
        } else {
            None
        }
    }
}
