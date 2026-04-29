use crate::error::LocationMap;
use crate::reader::SourceLoc;
use crate::value::Value;

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

    /// Enter an allocation region (scope boundary for allocator).
    /// No operands. Pushes a scope mark on the current FiberHeap.
    /// Effective for all fibers including root (after issue-525).
    RegionEnter,

    /// Exit an allocation region (scope boundary for allocator).
    /// No operands. Pops scope mark and releases scoped objects.
    /// Effective for all fibers including root (after issue-525).
    RegionExit,

    /// Exit a call-scoped allocation region.
    /// No operands. Pops two scope marks (barrier + region start),
    /// frees only the range between them (arg temporaries).
    RegionExitCall,

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

    /// Enter outbox routing context. No operands.
    /// Toggles allocation routing to the outbox (for yield-bound values).
    OutboxEnter,

    /// Exit outbox routing context. No operands.
    /// Reverts allocation routing to the private heap.
    OutboxExit,

    /// Push an explicit rotation frame. No operands.
    /// Captures the current heap state so `FlipSwap` can rotate relative
    /// to it and `FlipExit` can tear down this frame's swap pool without
    /// touching the caller's. Emitted at function entry when the function
    /// wants explicit rotation (e.g., a self-tail-recursive loop).
    FlipEnter,

    /// Rotate generations using the top flip frame. No operands.
    /// Equivalent to the trampoline's implicit `rotate_pools` but keyed
    /// off the flip stack. Emitted before a self-tail-call.
    FlipSwap,

    /// Pop the top flip frame and tear down its trailing swap pool. No
    /// operands. Emitted before every Return in a flip-wrapped function.
    FlipExit,

    /// Convert int → float. Pops value, pushes float. Identity on floats.
    IntToFloat,
    /// Convert float → int (truncation). Pops value, pushes int. Identity on ints.
    FloatToInt,
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

/// Disassemble bytecode and return one string per instruction
pub fn disassemble_lines(instructions: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut i = 0;

    while i < instructions.len() {
        let byte = instructions[i];
        let instr: Instruction = unsafe { std::mem::transmute(byte) };
        let mut line = format!("[{}] {:?}", i, instr);
        i += 1;

        match instr {
            Instruction::LoadConst if i + 1 < instructions.len() => {
                let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (const_idx={})", idx));
                i += 2;
            }
            Instruction::Jump | Instruction::JumpIfFalse | Instruction::JumpIfTrue
                if i + 3 < instructions.len() =>
            {
                let offset = i32::from_be_bytes([
                    instructions[i],
                    instructions[i + 1],
                    instructions[i + 2],
                    instructions[i + 3],
                ]);
                let target = (i + 4) as i64 + offset as i64;
                line.push_str(&format!(" (offset={}, target={})", offset, target));
                i += 4;
            }
            Instruction::LoadLocal | Instruction::StoreLocal if i + 1 < instructions.len() => {
                let index = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (index={})", index));
                i += 2;
            }
            Instruction::LoadUpvalue | Instruction::LoadUpvalueRaw | Instruction::StoreUpvalue
                if i + 2 < instructions.len() =>
            {
                let depth = instructions[i];
                let index = ((instructions[i + 1] as u16) << 8) | (instructions[i + 2] as u16);
                line.push_str(&format!(" (depth={}, index={})", depth, index));
                i += 3;
            }
            Instruction::Call | Instruction::TailCall if i + 1 < instructions.len() => {
                let arg_count = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (args={})", arg_count));
                i += 2;
            }
            Instruction::DupN if i < instructions.len() => {
                let offset = instructions[i];
                line.push_str(&format!(" (offset={})", offset));
                i += 1;
            }
            Instruction::MakeClosure if i + 3 < instructions.len() => {
                let const_idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                let num_captures =
                    ((instructions[i + 2] as u16) << 8) | (instructions[i + 3] as u16);
                line.push_str(&format!(
                    " (const_idx={}, num_captures={})",
                    const_idx, num_captures
                ));
                i += 4;
            }
            Instruction::ArrayMutRefDestructure
            | Instruction::ArrayMutSliceFrom
            | Instruction::ArrayMutRefOrNil
                if i + 1 < instructions.len() =>
            {
                let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (index={})", idx));
                i += 2;
            }
            Instruction::StructGetOrNil | Instruction::StructGetDestructure
                if i + 1 < instructions.len() =>
            {
                let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                line.push_str(&format!(" (const_idx={})", idx));
                i += 2;
            }
            Instruction::StructRest if i + 1 < instructions.len() => {
                let count = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                i += 2;
                let mut keys = Vec::new();
                for _ in 0..count {
                    if i + 1 < instructions.len() {
                        let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                        i += 2;
                        keys.push(format!("const[{}]", idx));
                    }
                }
                line.push_str(&format!(" (count={}, keys=[{}])", count, keys.join(", ")));
            }
            Instruction::Eval => {
                // No operands — pops 2 from stack, pushes 1
            }
            Instruction::ArrayMutExtend | Instruction::ArrayMutPush => {
                // No operands
            }
            Instruction::CallArrayMut | Instruction::TailCallArrayMut => {
                // No operands (arg count is dynamic, determined by array length)
            }
            Instruction::RegionEnter
            | Instruction::RegionExit
            | Instruction::RegionExitCall
            | Instruction::OutboxEnter
            | Instruction::OutboxExit
            | Instruction::FlipEnter
            | Instruction::FlipSwap
            | Instruction::FlipExit => {
                // No operands
            }
            Instruction::IntToFloat | Instruction::FloatToInt => {
                // No operands — pop one, push one
            }
            Instruction::PushParamFrame if i < instructions.len() => {
                let count = instructions[i];
                line.push_str(&format!(" (count={})", count));
                i += 1;
            }
            Instruction::PopParamFrame => {
                // No operands
            }
            _ => {}
        }

        lines.push(line);
    }

    lines
}

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
mod tests {
    use super::*;

    #[test]
    fn test_bytecode_emission() {
        let mut bc = Bytecode::new();
        bc.emit(Instruction::LoadConst);
        bc.emit_u16(0);
        bc.emit(Instruction::Return);
        assert_eq!(bc.instructions.len(), 4);
    }

    #[test]
    fn test_constant_deduplication() {
        let mut bc = Bytecode::new();
        let idx1 = bc.add_constant(Value::int(42));
        let idx2 = bc.add_constant(Value::int(42));
        assert_eq!(idx1, idx2);
        assert_eq!(bc.constants.len(), 1);
    }

    #[test]
    fn test_instruction_roundtrip() {
        // Test that all arithmetic/bitwise instructions can be emitted and decoded
        let instructions = [
            Instruction::Add,
            Instruction::Sub,
            Instruction::Mul,
            Instruction::Div,
            Instruction::Rem,
            Instruction::BitAnd,
            Instruction::BitOr,
            Instruction::BitXor,
            Instruction::BitNot,
            Instruction::Shl,
            Instruction::Shr,
        ];

        for instr in instructions {
            let mut bc = Bytecode::new();
            bc.emit(instr);
            let byte = bc.instructions[0];
            let decoded: Instruction = unsafe { std::mem::transmute(byte) };
            assert_eq!(decoded, instr, "Instruction {:?} did not roundtrip", instr);
        }
    }

    #[test]
    fn test_region_instruction_roundtrip() {
        for instr in [Instruction::RegionEnter, Instruction::RegionExit] {
            let mut bc = Bytecode::new();
            bc.emit(instr);
            assert_eq!(
                bc.instructions.len(),
                1,
                "Region instruction should be 1 byte"
            );
            let decoded: Instruction = unsafe { std::mem::transmute(bc.instructions[0]) };
            assert_eq!(decoded, instr, "Instruction {:?} did not roundtrip", instr);
        }
    }

    #[test]
    fn test_bytecode_variants_distinct() {
        // Catch accidental duplication of variants (they all get auto-
        // numbered by the compiler, so any duplicate would be a compile
        // error anyway — but this test additionally guards against a
        // refactor that collapses two variants into one). All repr values
        // must be distinct; pick a few representative ones and spot-check.
        assert_ne!(
            Instruction::StructGetDestructure as u8,
            Instruction::StructGetOrNil as u8,
            "StructGetDestructure must have a distinct byte value from StructGetOrNil"
        );
        assert_ne!(
            Instruction::FirstDestructure as u8,
            Instruction::RestDestructure as u8,
        );
        assert_ne!(
            Instruction::OutboxEnter as u8,
            Instruction::OutboxExit as u8,
        );
    }

    #[test]
    fn test_region_disassembly() {
        let mut bc = Bytecode::new();
        bc.emit(Instruction::RegionEnter);
        bc.emit(Instruction::RegionExit);
        let lines = disassemble_lines(&bc.instructions);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("RegionEnter"));
        assert!(lines[1].contains("RegionExit"));
    }
}
