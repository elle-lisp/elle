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

    /// Pop value from stack
    Pop,

    /// Duplicate top of stack
    Dup,

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

    /// Vector construction (size)
    MakeVector,

    /// Vector ref (index)
    VectorRef,

    /// Vector set (index)
    VectorSet,

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

    /// Comparisons
    Eq,
    Lt,
    Gt,
    Le,
    Ge,

    /// Type checks
    IsNil,
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
}

impl Bytecode {
    pub fn new() -> Self {
        Bytecode {
            instructions: Vec::new(),
            constants: Vec::new(),
            inline_caches: std::collections::HashMap::new(),
        }
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
}

impl Default for Bytecode {
    fn default() -> Self {
        Self::new()
    }
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
        let idx1 = bc.add_constant(Value::Int(42));
        let idx2 = bc.add_constant(Value::Int(42));
        assert_eq!(idx1, idx2);
        assert_eq!(bc.constants.len(), 1);
    }
}
