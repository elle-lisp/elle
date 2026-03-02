use crate::error::LocationMap;
use crate::reader::SourceLoc;
use crate::value::Value;

/// Bytecode instruction set
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Instruction {
    /// Load constant from constant pool
    LoadConst,

    /// Load local variable (depth, index)
    LoadLocal,

    /// Load global variable
    LoadGlobal,

    /// Store local variable (depth, index)
    StoreLocal,

    /// Store global variable
    StoreGlobal,

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

    /// Jump unconditionally (offset i16)
    Jump,

    /// Jump if false (offset i16)
    JumpIfFalse,

    /// Jump if true (offset i16)
    JumpIfTrue,

    /// Create closure (const_idx, num_upvalues)
    MakeClosure,

    /// Cons cell construction
    Cons,

    /// Car operation
    Car,

    /// Cdr operation
    Cdr,

    /// Array construction (size)
    MakeArray,

    /// Array ref (index)
    ArrayRef,

    /// Array set (index)
    ArraySet,

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

    /// Scope management instructions (Phase 2)
    /// Push a new scope (scope_type u8)
    PushScope,

    /// Pop the current scope
    PopScope,

    /// Define local variable (symbol_idx u16)
    DefineLocal,

    /// Wrap value in a cell for shared mutable access (Phase 4)
    /// Pops value from stack, wraps it in a cell, pushes the cell
    MakeCell,

    /// Unwrap a cell to get its value
    UnwrapCell,

    /// Update a cell's value
    UpdateCell,

    /// Yield (suspends execution)
    Yield,

    /// Empty list constant
    EmptyList,

    /// Car with silent nil (for destructuring): returns nil if not a cons
    CarOrNil,

    /// Cdr with silent nil (for destructuring): returns nil if not a cons
    CdrOrNil,

    /// Array/tuple ref with silent nil (for destructuring): returns nil if out of bounds
    /// Operand: u16 index (immediate)
    ArrayRefOrNil,
    /// Array/tuple slice from index (for & rest destructuring): returns sub-array from index to end
    /// Operand: u16 index (immediate)
    ArraySliceFrom,

    /// Type check: is value a tuple?
    IsTuple,
    /// Type check: is value an array?
    IsArray,
    /// Type check: is value a struct?
    IsStruct,
    /// Type check: is value a table?
    IsTable,
    /// Get array length as integer
    ArrayLen,
    /// Table/struct get with silent nil (for destructuring): returns nil if key missing or wrong type.
    /// Operand: u16 constant pool index (keyword key)
    TableGetOrNil,

    /// Runtime eval: pop expr and env from stack, compile+execute, push result.
    Eval,

    /// Extend array with elements of another indexed type (for splice).
    /// Pops source, pops array, pushes extended array.
    ArrayExtend,
    /// Push a single value onto an array (for splice).
    /// Pops value, pops array, pushes array with value appended.
    ArrayPush,
    /// Call function with elements of an array as arguments (for splice).
    /// Pops args array, pops function, calls function with array elements.
    CallArray,
    /// Tail call with elements of an array as arguments (for splice).
    /// Pops args array, pops function, tail calls with array elements.
    TailCallArray,
}

/// Inline cache entry for function lookups
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub symbol_id: u32,
    pub cached_value: Option<Value>,
}

/// Compiled bytecode with constants
#[derive(Debug, Clone)]
pub struct Bytecode {
    pub instructions: Vec<u8>,
    pub constants: Vec<Value>,
    pub inline_caches: std::collections::HashMap<usize, CacheEntry>,
    /// Symbol ID → name mapping for cross-thread portability.
    /// When bytecode is sent to a new thread, symbol IDs may differ.
    /// This map allows remapping globals to the correct IDs.
    pub symbol_names: std::collections::HashMap<u32, String>,
    /// Bytecode offset → source location mapping for error reporting.
    /// Maps instruction offsets to their source locations.
    pub location_map: LocationMap,
}

impl Bytecode {
    pub fn new() -> Self {
        Bytecode {
            instructions: Vec::new(),
            constants: Vec::new(),
            inline_caches: std::collections::HashMap::new(),
            symbol_names: std::collections::HashMap::new(),
            location_map: LocationMap::new(),
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

    /// Get current position for jump patching
    pub fn current_pos(&self) -> usize {
        self.instructions.len()
    }

    /// Patch a jump instruction at a given position
    pub fn patch_jump(&mut self, pos: usize, offset: i16) {
        self.instructions[pos] = (offset >> 8) as u8;
        self.instructions[pos + 1] = (offset & 0xff) as u8;
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
            Instruction::LoadConst | Instruction::LoadGlobal | Instruction::StoreGlobal => {
                if i + 1 < instructions.len() {
                    let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                    line.push_str(&format!(" (const_idx={})", idx));
                    i += 2;
                }
            }
            Instruction::Jump | Instruction::JumpIfFalse | Instruction::JumpIfTrue => {
                if i + 1 < instructions.len() {
                    let high = instructions[i] as i8 as i16;
                    let low = instructions[i + 1] as i16;
                    let offset = (high << 8) | (low & 0xFF);
                    let target = (i + 2) as i32 + offset as i32;
                    line.push_str(&format!(" (offset={}, target={})", offset, target));
                    i += 2;
                }
            }
            Instruction::LoadLocal | Instruction::StoreLocal => {
                if i + 1 < instructions.len() {
                    let depth = instructions[i];
                    let index = instructions[i + 1];
                    line.push_str(&format!(" (depth={}, index={})", depth, index));
                    i += 2;
                }
            }
            Instruction::LoadUpvalue | Instruction::LoadUpvalueRaw | Instruction::StoreUpvalue => {
                if i + 1 < instructions.len() {
                    let depth = instructions[i];
                    let index = instructions[i + 1];
                    line.push_str(&format!(" (depth={}, index={})", depth, index));
                    i += 2;
                }
            }
            Instruction::Call | Instruction::TailCall => {
                if i < instructions.len() {
                    let arg_count = instructions[i];
                    line.push_str(&format!(" (args={})", arg_count));
                    i += 1;
                }
            }
            Instruction::DupN => {
                if i < instructions.len() {
                    let offset = instructions[i];
                    line.push_str(&format!(" (offset={})", offset));
                    i += 1;
                }
            }
            Instruction::MakeClosure => {
                if i + 2 < instructions.len() {
                    let const_idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                    let num_captures = instructions[i + 2];
                    line.push_str(&format!(
                        " (const_idx={}, num_captures={})",
                        const_idx, num_captures
                    ));
                    i += 3;
                }
            }
            Instruction::ArrayRefOrNil | Instruction::ArraySliceFrom => {
                if i + 1 < instructions.len() {
                    let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                    line.push_str(&format!(" (index={})", idx));
                    i += 2;
                }
            }
            Instruction::TableGetOrNil => {
                if i + 1 < instructions.len() {
                    let idx = ((instructions[i] as u16) << 8) | (instructions[i + 1] as u16);
                    line.push_str(&format!(" (const_idx={})", idx));
                    i += 2;
                }
            }
            Instruction::Eval => {
                // No operands — pops 2 from stack, pushes 1
            }
            Instruction::ArrayExtend | Instruction::ArrayPush => {
                // No operands
            }
            Instruction::CallArray | Instruction::TailCallArray => {
                // No operands (arg count is dynamic, determined by array length)
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
}
