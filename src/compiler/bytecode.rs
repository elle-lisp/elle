use crate::error::LocationMap;
use crate::reader::SourceLoc;
use crate::value::Value;

mod disasm;
pub use disasm::*;

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

    /// Function call (arg_count)
    Call,

    /// Tail call (arg_count)
    TailCall,

    /// Return from function
    Return,

    /// Jump unconditionally (offset i32)
    Jump,

    /// Jump if false (offset i32)
    JumpIfFalse,

    /// Jump if true (offset i32)
    JumpIfTrue,

    /// Create closure (const_idx, num_upvalues)
    MakeClosure,

    /// Pair cell construction
    Pair,

    /// First operation
    First,

    /// Rest operation
    Rest,

    /// Array construction (size)
    MakeArrayMut,

    /// Array ref (index)
    ArrayMutRef,

    /// Array set (index)
    ArrayMutSet,

    /// Specialized arithmetic operations
    AddInt,
    SubInt,
    MulInt,
    DivInt,

    /// Generic arithmetic (handles floats)
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

    /// Wrap value in a capture cell for shared mutable access (Phase 4)
    /// Pops value from stack, wraps it in a capture cell, pushes the cell
    MakeCapture,

    /// Unwrap a capture cell to get its value
    UnwrapCapture,

    /// Update a capture cell's value
    UpdateCapture,

    /// Emit a signal (suspends execution). Operand: u16 signal bits.
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
    /// Pops args array, pops function, calls function with array elements.
    CallArrayMut,
    /// Tail call with elements of an array as arguments (for splice).
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
    /// Operand: u32 allowed_bits.
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

    // === New intrinsic opcodes ===
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
    /// Mutable → immutable copy
    IntrFreeze,
    /// Immutable → mutable copy
    IntrThaw,
    /// Bitwise tag+payload equality (pops b, pops a, pushes bool)
    Identical,

    /// Arity-checked function call (arg_count). Compiler verified arity.
    CallChecked,
    /// Arity-checked tail call (arg_count). Compiler verified arity.
    TailCallChecked,

    /// Append string to @string (pops value, pops string, pushes string)
    IntrStringPush,
    /// Append byte to @bytes (pops value, pops bytes, pushes bytes)
    IntrBytesPush,

    /// Increment the reference count of a region.
    /// Operand: u32 region_id.
    /// Emitted when a value in region A is stored into a structure in
    /// region B — region A must outlive region B's free point.
    IncrefRegion,

    /// Decrement the reference count of a region.
    /// Operand: u32 region_id.
    /// Decrements RC; when RC hits 0, the region's pages are freed and
    /// cascade decrefs fire for any cross-region references found in
    /// the region's contents. The sole region-demise bytecode.
    DecrefRegion,

    /// Decrement the reference count of the region of the value on
    /// top of the operand stack. No operand. Pops the value and
    /// calls `region_of` + `decref_region` at runtime. Used by the
    /// caller at a Call-result region's decref_point when the compile-time
    /// region ID doesn't match the runtime region of the actual
    /// returned value.
    DecrefValueRegion,

    /// Decrement the reference count of the region of the value on top of
    /// the operand stack, using `region_of` (NOT `result_region_of`). No
    /// operand. Pops the value. Unlike `DecrefValueRegion`, this does NOT
    /// see through a `CaptureCell` wrapper — it frees the CELL's own region.
    /// Emitted at a captured (env-allocated) binding's `decref_point` to
    /// release the per-value env cell `populate_env` minted for it (the
    /// owned-binding release for capture cells; docs/impl/region/rules.md Rule 8).
    /// `DecrefValueRegion` would unwrap to the inner value's region instead —
    /// freeing a caller-owned region and leaking the cell.
    DecrefCellRegion,

    /// Increment the reference count of the region of the value on
    /// top of the operand stack. No operand. Pops the value and calls
    /// `region_of` + `incref_region` at runtime (skipping region 0).
    /// The mirror of `DecrefValueRegion`; emitted at a function's tail
    /// value so the callee hands the caller one owning reference to the
    /// result's runtime region.
    IncrefValueRegion,

    /// Adopt the region of one value into another's Owned subtree (the
    /// `AdoptRegion` LIR instruction). No operand. Pops two values — `child`
    /// (top) then `parent` — resolves each to its runtime region via
    /// `result_region_of`, and calls `RegionStore::adopt_region(parent_region,
    /// child_region)`, freezing the child's RC so it is reclaimed only by the
    /// parent's subtree drop (docs/impl/region/ownership.md § "Adoption and subtree
    /// drop"). Emitted by the ownership forest; realized on the
    /// interpreter and the JIT (`elle_jit_adopt_region`).
    AdoptRegion,

    /// Adopt the region of one value into another's Owned subtree, resolving
    /// BOTH operands with `region_of` — NOT `result_region_of` (the
    /// `AdoptCellRegion` LIR instruction). No operand. Pops `child` (top) then
    /// `parent`, resolves each to its runtime region via `region_of` (so a
    /// `CaptureCell` operand is NOT unwrapped — its OWN region is used), and calls
    /// `RegionStore::adopt_region`. This is the only ownership cut that can name a
    /// capture cell's own region, letting the forest reclaim a cell↔closure clique
    /// as a unit (docs/impl/region/adopt.md § "The capture adopt"). Emitted by the
    /// ownership forest; realized on the interpreter (`handle_adopt_cell_region`)
    /// and the JIT (`elle_jit_adopt_cell_region`).
    AdoptCellRegion,

    /// Debug-only region-coalescing oracle (the `AssertRegionMatches` LIR
    /// instruction). Operand: u32 region slot. Peeks the value on top of the
    /// operand stack (does NOT pop — it is the return value), resolves the slot
    /// through the current `activation_region_map`, and under `debug_assertions`
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
    /// drop at the activation's normal completion (docs/impl/region/owner.md
    /// § "Owner nodes — an activation as a forest root"). An immediate child
    /// (no region) adopts nothing and mints no node. Realized on the
    /// interpreter and the JIT (`elle_jit_adopt_into_activation`).
    AdoptIntoActivation,

    /// Materialize a heap literal — a string, or quoted compound data (list /
    /// array / nested structure) — into a fresh per-activation region. Operands:
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

/// Compiled bytecode with constants
#[derive(Debug, Clone)]
pub struct Bytecode {
    pub instructions: Vec<u8>,
    pub constants: Vec<Value>,
    /// Symbol ID → name mapping for cross-thread portability.
    /// When bytecode is sent to a new thread, symbol IDs may differ.
    /// This map allows remapping globals to the correct IDs.
    pub symbol_names: std::collections::HashMap<u32, String>,
    /// Bytecode offset → source location mapping for error reporting.
    /// Maps instruction offsets to their source locations.
    pub location_map: LocationMap,
    /// Inferred signal of the top-level expression. Carried through the
    /// pipeline so that `execute_scheduled` can build a thunk with the
    /// correct signal metadata for fiber scheduling and shared allocator
    /// provisioning.
    pub signal: crate::signals::Signal,
    /// Signal projection: maps keyword field names to the signals of exported
    /// closures. Populated by `compute_signal_projection` during file-scope
    /// compilation. When an importing file sees `module:field`, the analyzer
    /// uses this projection instead of the conservative `Polymorphic` fallback.
    pub signal_projection: Option<std::collections::HashMap<String, crate::signals::Signal>>,
    /// Blueprints for this code object's `MakeClosure` instructions. Each
    /// `MakeClosure` pushes its nested-lambda template here and emits the index;
    /// the VM/JIT materialize a fresh region-allocated `HeapObject::ClosureTemplate`
    /// per execution (a heap literal is an ordinary, reclaimable allocation).
    /// Threaded into the executing `Code` so each template is reclaimed by region
    /// RC, never pinned for the process lifetime.
    pub child_protos: Vec<std::rc::Rc<crate::value::closure::ClosureTemplate>>,
    /// The static region slots this (top-level / entry) function's allocations
    /// SHARE after a builder-idiom merge (docs/impl/region/merging.md § Merging),
    /// carried from the entry `LirFunction.merged_slots` so the executing `Code`
    /// can mint-or-reuse them. The per-lambda equivalent rides
    /// `ClosureTemplate.merged_slots`; this is the entry-function path
    /// (`Bytecode → Code`), which would otherwise read empty. Empty unless a merge
    /// fired (a builder idiom seeded by a nested `%pair` literal), so inert when
    /// no merge exists.
    pub merged_slots: std::rc::Rc<rustc_hash::FxHashSet<u32>>,
}

impl Bytecode {
    pub fn new() -> Self {
        Bytecode {
            instructions: Vec::new(),
            constants: Vec::new(),
            symbol_names: std::collections::HashMap::new(),
            location_map: LocationMap::new(),
            signal: crate::signals::Signal::silent(),
            signal_projection: None,
            child_protos: Vec::new(),
            merged_slots: crate::value::code::empty_merged_slots(),
        }
    }

    /// Record a source location for the current bytecode position.
    /// Only records non-synthetic spans (line > 0).
    pub fn record_location(&mut self, span: &crate::syntax::Span) {
        // Skip synthetic spans (all zeros)
        if span.line == 0 && span.col == 0 && span.start == 0 && span.end == 0 {
            return;
        }

        let offset = self.current_pos();
        let loc = SourceLoc::new(
            span.file.clone().unwrap_or_else(|| "<input>".to_string()),
            span.line as usize,
            span.col as usize,
        );
        self.location_map.insert(offset, loc);
    }

    /// Add a symbol constant and record its name for portability.
    /// This enables cross-thread symbol ID remapping.
    pub fn add_symbol(&mut self, id: u32, name: &str) -> u16 {
        self.symbol_names
            .entry(id)
            .or_insert_with(|| name.to_string());
        self.add_constant(Value::symbol(id))
    }

    /// Add a constant and return its index
    pub fn add_constant(&mut self, value: Value) -> u16 {
        // Check if constant already exists
        for (i, c) in self.constants.iter().enumerate() {
            if c == &value {
                return i as u16;
            }
        }

        let idx = self.constants.len();
        if idx > u16::MAX as usize {
            panic!("Too many constants");
        }
        self.constants.push(value);
        idx as u16
    }

    /// Emit an instruction
    pub fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr as u8);
    }

    /// Emit a byte
    pub fn emit_byte(&mut self, byte: u8) {
        self.instructions.push(byte);
    }

    /// Emit a u16 (big-endian)
    pub fn emit_u16(&mut self, value: u16) {
        self.instructions.push((value >> 8) as u8);
        self.instructions.push((value & 0xff) as u8);
    }

    /// Emit a u32 (big-endian). Used for region-id operands, which are
    /// minted fresh per allocation-site and thus need the full 32-bit
    /// space (see docs/regions/semantics.md — every value its own region).
    pub fn emit_u32(&mut self, value: u32) {
        self.instructions.push((value >> 24) as u8);
        self.instructions.push((value >> 16) as u8);
        self.instructions.push((value >> 8) as u8);
        self.instructions.push((value & 0xff) as u8);
    }

    /// Emit an i16 (big-endian)
    pub fn emit_i16(&mut self, value: i16) {
        self.emit_u16(value as u16);
    }

    /// Emit an i32 (big-endian)
    pub fn emit_i32(&mut self, value: i32) {
        let bytes = value.to_be_bytes();
        self.instructions.push(bytes[0]);
        self.instructions.push(bytes[1]);
        self.instructions.push(bytes[2]);
        self.instructions.push(bytes[3]);
    }

    /// Get current position for jump patching
    pub fn current_pos(&self) -> usize {
        self.instructions.len()
    }

    /// Patch a jump instruction at a given position (i32 big-endian)
    pub fn patch_jump(&mut self, pos: usize, offset: i32) {
        let bytes = offset.to_be_bytes();
        self.instructions[pos] = bytes[0];
        self.instructions[pos + 1] = bytes[1];
        self.instructions[pos + 2] = bytes[2];
        self.instructions[pos + 3] = bytes[3];
    }

    pub fn patch_u16(&mut self, pos: usize, value: u16) {
        self.instructions[pos] = (value >> 8) as u8;
        self.instructions[pos + 1] = (value & 0xff) as u8;
    }
}

impl Default for Bytecode {
    fn default() -> Self {
        Self::new()
    }
}

// ── Debug formatting ────────────────────────────────────────────────

/// Disassemble bytecode with proper instruction names and operands
pub fn disassemble(instructions: &[u8]) -> String {
    disassemble_lines(instructions)
        .iter()
        .map(|line| format!("  {}", line))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Pretty print bytecode with constants
pub fn format_bytecode_with_constants(instructions: &[u8], constants: &[crate::Value]) -> String {
    let mut output = String::new();
    output.push_str("Bytecode:\n");
    output.push_str(&disassemble(instructions));
    output.push_str("\nConstants:\n");
    for (i, c) in constants.iter().enumerate() {
        output.push_str(&format!("  [{}] = {:?}\n", i, c));
    }
    output
}

#[cfg(test)]
mod tests;
