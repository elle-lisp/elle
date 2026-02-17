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

    // Exception handling instructions (Phase 3)
    /// Push handler frame for handler-case (handler_offset i16, finally_offset i16 or -1)
    PushHandler,

    /// Pop handler frame
    PopHandler,

    /// Create handler context (handler_fn_idx, condition_id)
    CreateHandler,

    /// Check if exception occurred
    /// Used in handler code to verify exception is still set
    /// (Only reached if an exception occurred)
    CheckException,

    /// Match exception against handler exception ID (compares stack top with current exception's ID)
    MatchException,

    /// Bind caught exception to variable (var_symbol_id u16)
    BindException,

    /// Load current exception onto stack
    LoadException,

    /// Clear current exception state
    ClearException,

    /// Invoke a restart by name (restart_name_id u16)
    InvokeRestart,

    /// Yield from coroutine (suspends execution)
    Yield,

    /// Empty list constant
    EmptyList,

    /// Re-raise current exception: pop handler, clear handling_exception flag,
    /// but leave current_exception set so the interrupt mechanism re-fires.
    ReraiseException,
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
}

impl Bytecode {
    pub fn new() -> Self {
        Bytecode {
            instructions: Vec::new(),
            constants: Vec::new(),
            inline_caches: std::collections::HashMap::new(),
            symbol_names: std::collections::HashMap::new(),
        }
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
}
