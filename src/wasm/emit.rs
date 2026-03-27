//! LIR → WASM emission.
//!
//! Converts a `LirFunction` into WASM module bytes using `wasm-encoder`.
//! Each LIR register maps to two WASM locals (tag: i64, payload: i64).
//! Immediate values (int, float, nil, bool) are constructed in WASM.
//! Heap operations go through host function calls.
//!
//! Control flow: LIR basic blocks are emitted recursively. Branch terminators
//! become WASM `if/else/end`. Jump terminators inline the target block.
//!
//! Heap constants (strings, NativeFn, closures from ValueConst) are collected
//! into a constant pool during emission. The host pre-loads them into the
//! handle table at instantiation time.

use crate::lir::{BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Reg, Terminator, UnaryOp};
use crate::value::keyword::intern_keyword;
use crate::value::repr::*;
use crate::value::Value;
use std::collections::HashMap;
use wasm_encoder::*;

/// Result of WASM emission: module bytes + constant pool.
pub struct EmitResult {
    /// Raw WASM module bytes.
    pub wasm_bytes: Vec<u8>,
    /// Heap constants referenced by the module. The host must load these
    /// into its handle table before execution, and `rt_load_const(i)`
    /// returns the i-th constant.
    pub const_pool: Vec<Value>,
}

/// Emit a WASM module from a top-level LirFunction.
pub fn emit_module(func: &LirFunction) -> EmitResult {
    let mut emitter = WasmEmitter::new();
    emitter.emit_module(func)
}

// Host function import indices (in order of declaration)
const _FN_CALL_PRIMITIVE: u32 = 0;
const FN_RT_CALL: u32 = 1;
const FN_RT_LOAD_CONST: u32 = 2;
const FN_RT_DATA_OP: u32 = 3;
const FN_RT_MAKE_CLOSURE: u32 = 4;
const FN_RT_PUSH_PARAM: u32 = 5;
const FN_RT_POP_PARAM: u32 = 6;
const FN_RT_PREPARE_TAIL_CALL: u32 = 7;

// First non-imported function index
const FN_ENTRY: u32 = 8;

// Linear memory layout
const ARGS_BASE: i32 = 256; // Args buffer starts at byte 256

// Data operation codes for rt_data_op
const OP_CONS: i32 = 0;
const OP_CAR: i32 = 1;
const OP_CDR: i32 = 2;
const OP_CAR_DESTRUCTURE: i32 = 3;
const OP_CDR_DESTRUCTURE: i32 = 4;
const OP_CAR_OR_NIL: i32 = 5;
const OP_CDR_OR_NIL: i32 = 6;
const OP_MAKE_ARRAY: i32 = 7;
const OP_MAKE_LBOX: i32 = 8;
const OP_LOAD_LBOX: i32 = 9;
const OP_STORE_LBOX: i32 = 10;
const _OP_MAKE_STRING: i32 = 11;
const OP_ARRAY_REF_DESTRUCTURE: i32 = 12;
const OP_ARRAY_SLICE_FROM: i32 = 13;
const OP_STRUCT_GET_OR_NIL: i32 = 14;
const OP_STRUCT_GET_DESTRUCTURE: i32 = 15;
const OP_ARRAY_EXTEND: i32 = 16;
const OP_ARRAY_PUSH: i32 = 17;
const OP_ARRAY_LEN: i32 = 18;
const OP_ARRAY_REF_OR_NIL: i32 = 19;
const OP_STRUCT_REST: i32 = 20;

struct WasmEmitter {
    label_to_idx: HashMap<Label, usize>,
    num_regs: u32,
    /// Whether we're currently emitting a closure function (vs entry).
    is_closure: bool,
    /// Heap constants collected during emission.
    const_pool: Vec<Value>,
    /// Maps nested LirFunction pointer → table index.
    /// Populated during collect_nested_functions, used by MakeClosure emission.
    closure_table_idx: HashMap<*const LirFunction, u32>,
    /// Offset for register locals (0 for entry, 4 for closures with params).
    local_offset: u32,
    /// WASM local index for signal scratch (i32).
    signal_local: u32,
    /// Number of stack-local slots (non-LBox let-bound variables in closures).
    num_stack_locals: u32,
}

impl WasmEmitter {
    fn new() -> Self {
        WasmEmitter {
            label_to_idx: HashMap::new(),
            num_regs: 0,
            is_closure: false,
            const_pool: Vec::new(),
            closure_table_idx: HashMap::new(),
            local_offset: 0,
            signal_local: 0,
            num_stack_locals: 0,
        }
    }

    fn emit_module(&mut self, func: &LirFunction) -> EmitResult {
        // Phase 1: collect all nested LirFunctions by walking MakeClosure instructions.
        // Each gets a table index. The entry function is NOT in the table.
        let mut nested_funcs: Vec<&LirFunction> = Vec::new();
        collect_nested_functions(func, &mut nested_funcs);
        let num_closures = nested_funcs.len() as u32;

        // Build pointer → table index map for MakeClosure emission
        self.closure_table_idx.clear();
        for (i, nf) in nested_funcs.iter().enumerate() {
            self.closure_table_idx
                .insert(*nf as *const LirFunction, i as u32);
        }

        let mut module = Module::new();

        // Type section
        let mut types = TypeSection::new();
        // Type 0: entry function () -> (i64, i64)
        types.ty().function([], [ValType::I64, ValType::I64]);
        // Type 1: call_primitive(prim_id, args_ptr, nargs, ctx) -> (tag, payload, signal)
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // Type 2: rt_call(func_tag, func_payload, args_ptr, nargs, ctx) -> (tag, payload, signal)
        types.ty().function(
            [
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // Type 3: rt_load_const(index) -> (tag, payload)
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        // Type 4: rt_data_op(op, args_ptr, nargs) -> (tag, payload, signal)
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // Type 5: closure function (env_ptr, args_ptr, nargs, ctx) -> (tag, payload)
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64],
        );
        // Type 6: rt_make_closure(table_idx, captures_ptr, metadata_ptr) -> (tag, payload)
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64],
        );
        // Type 7: rt_push_param(args_ptr, npairs) -> ()
        types.ty().function([ValType::I32, ValType::I32], []);
        // Type 8: rt_pop_param() -> ()
        types.ty().function([], []);
        // Type 9: rt_prepare_tail_call(func_tag, func_payload, args_ptr, nargs, env_ptr)
        //   -> (env_ptr, table_idx, is_wasm, tag, payload, signal)
        types.ty().function(
            [
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I64,
                ValType::I64,
                ValType::I32,
            ],
        );
        module.section(&types);

        // Import section (function indices 0–7)
        let mut imports = ImportSection::new();
        imports.import("elle", "call_primitive", EntityType::Function(1));
        imports.import("elle", "rt_call", EntityType::Function(2));
        imports.import("elle", "rt_load_const", EntityType::Function(3));
        imports.import("elle", "rt_data_op", EntityType::Function(4));
        imports.import("elle", "rt_make_closure", EntityType::Function(6));
        imports.import("elle", "rt_push_param", EntityType::Function(7));
        imports.import("elle", "rt_pop_param", EntityType::Function(8));
        imports.import("elle", "rt_prepare_tail_call", EntityType::Function(9));
        module.section(&imports);

        // Function section: entry (type 0) + closures (type 5 each)
        let mut functions = FunctionSection::new();
        functions.function(0); // entry function
        for _ in 0..num_closures {
            functions.function(5); // closure function
        }
        module.section(&functions);

        // Table section: funcref table for closure indirect calls
        if num_closures > 0 {
            let mut tables = TableSection::new();
            tables.table(TableType {
                element_type: RefType::FUNCREF,
                minimum: num_closures as u64,
                maximum: Some(num_closures as u64),
                shared: false,
                table64: false,
            });
            module.section(&tables);
        }

        // Memory section
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        // Export section
        let mut exports = ExportSection::new();
        exports.export("__elle_entry", ExportKind::Func, FN_ENTRY);
        exports.export("__elle_memory", ExportKind::Memory, 0);
        if num_closures > 0 {
            exports.export("__elle_table", ExportKind::Table, 0);
        }
        module.section(&exports);

        // Element section: initialize table with closure function refs
        if num_closures > 0 {
            let mut elements = ElementSection::new();
            // Table index 0, offset 0, function indices starting at FN_ENTRY + 1
            let func_indices: Vec<u32> = (0..num_closures).map(|i| FN_ENTRY + 1 + i).collect();
            elements.active(
                Some(0),
                &ConstExpr::i32_const(0),
                Elements::Functions(func_indices.into()),
            );
            module.section(&elements);
        }

        // Code section: emit entry function + all closure functions
        let mut code = CodeSection::new();

        // Entry function body
        let entry_body = self.emit_function(func);
        code.function(&entry_body);

        // Closure function bodies
        for nested in &nested_funcs {
            let closure_body = self.emit_closure_function(nested);
            code.function(&closure_body);
        }

        module.section(&code);

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
        }
    }

    fn emit_function(&mut self, func: &LirFunction) -> Function {
        self.label_to_idx.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        self.num_regs = func.num_regs;
        self.local_offset = 0;
        self.is_closure = false;
        // Locals: tags [0,N), payloads [N,2N), env_ptr (2N), signal/state (2N+1)
        self.signal_local = func.num_regs * 2 + 1;

        let mut f = Function::new([
            (func.num_regs, ValType::I64), // tags
            (func.num_regs, ValType::I64), // payloads
            (1, ValType::I32),             // env_ptr
            (1, ValType::I32),             // signal/state scratch
        ]);

        self.emit_cfg(&mut f, func);

        f.instruction(&Instruction::End);
        f
    }

    /// Emit a control flow graph using loop + br_table dispatch.
    ///
    /// Each LIR basic block becomes a case in a br_table. A `$state` local
    /// tracks which block to execute next. Return terminators break out of
    /// the loop. Jump and Branch terminators set `$state` and continue.
    ///
    /// This handles any CFG topology (including cond's deeply nested
    /// if/else patterns) without needing merge-point analysis.
    fn emit_cfg(&mut self, f: &mut Function, func: &LirFunction) {
        let num_blocks = func.blocks.len();
        if num_blocks == 0 {
            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
            f.instruction(&Instruction::I64Const(0));
            return;
        }

        // Single block: no loop needed
        if num_blocks == 1 {
            let block = &func.blocks[0];
            for spanned in &block.instructions {
                self.emit_instr(f, &spanned.instr);
            }
            self.emit_terminator_return(f, &block.terminator.terminator);
            return;
        }

        // State local: reuse signal_local as state (i32).
        // We'll use a separate local for state to avoid conflicts.
        // State local is at self.local_offset + 2*num_regs + 1 for closures,
        // or 2*num_regs + 1 for entry. But signal_local is already there.
        // Add another i32 local for state... actually signal_local IS an i32.
        // Let's just use signal_local as the state variable (it's only used
        // transiently during call emission, and the loop dispatch doesn't
        // overlap with those uses).
        let state_local = self.signal_local;

        // Initialize state to entry block index
        let entry_idx = self.label_to_idx[&func.entry] as i32;
        f.instruction(&Instruction::I32Const(entry_idx));
        f.instruction(&Instruction::LocalSet(state_local));

        // Outer block for return (br 0 from inside loop = break to after block)
        // loop $dispatch
        //   block $b0
        //     block $b1
        //       ...
        //         block $bN
        //           br_table $b0 $b1 ... $bN (local.get $state)
        //         end  ;; $bN
        //       ...
        //     end  ;; $b1
        //   end  ;; $b0
        //   ;; block 0 code here (br_table jumps to $b0 which is the outermost block
        //   ;;   so after its `end` we fall through to block 0's code)
        //   ... set state, br $dispatch ...
        // end  ;; loop

        // Structure: block { loop { block*N { br_table } code_N; code_N-1; ... code_0; br loop } }

        // The br_table maps index i to block depth. Block depth 0 = innermost,
        // N-1 = outermost of the nested blocks. After landing at block i's end,
        // execution falls through to block i's code.

        // Loop for dispatch. All exits use `return`.
        f.instruction(&Instruction::Loop(BlockType::Empty));

        // Nested blocks for br_table targets (innermost = highest index)
        for _ in 0..num_blocks {
            f.instruction(&Instruction::Block(BlockType::Empty));
        }

        // br_table dispatch
        f.instruction(&Instruction::LocalGet(state_local));
        // Targets: block 0 is outermost (depth = num_blocks - 1),
        // block N-1 is innermost (depth = 0).
        // br_table target for state=i should jump to depth (num_blocks - 1 - i).
        let targets: Vec<u32> = (0..num_blocks as u32)
            .map(|i| num_blocks as u32 - 1 - i)
            .collect();
        // Default target = first block
        f.instruction(&Instruction::BrTable(targets.clone().into(), targets[0]));

        // After br_table, emit blocks in reverse order (innermost end first)
        // Block N-1's code comes right after the innermost block's end
        for block_idx in (0..num_blocks).rev() {
            f.instruction(&Instruction::End); // close the block

            let block = &func.blocks[block_idx];

            // Emit instructions
            for spanned in &block.instructions {
                self.emit_instr(f, &spanned.instr);
            }

            // Emit terminator
            match &block.terminator.terminator {
                Terminator::Return(reg) => {
                    f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                    // Store into the outer result blocks and break out
                    // br 1 goes to the inner result block, br 2 to the outer
                    // We need to break out of the loop (depth: num_blocks - block_idx + loop_depth)
                    // Actually: the structure is:
                    //   block(result i64) $ret_tag    depth from here: varies
                    //     block(result i64) $ret_pay
                    //       loop $dispatch
                    //         block*N
                    //         ...
                    //         code
                    // From code position, depths are:
                    //   0 = previous block's block
                    //   ... up to num_blocks-1-block_idx = our block
                    //   num_blocks - block_idx = loop
                    //   num_blocks - block_idx + 1 = $ret_pay
                    //   num_blocks - block_idx + 2 = $ret_tag
                    // We want to br to $ret_pay with payload, then that falls through to ret_tag
                    // Actually, multi-value br doesn't work like that.

                    // Simpler: just use `return` instruction which works from anywhere.
                    f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                    f.instruction(&Instruction::Return);
                }
                Terminator::Jump(target) => {
                    let target_idx = self.label_to_idx[target] as i32;
                    f.instruction(&Instruction::I32Const(target_idx));
                    f.instruction(&Instruction::LocalSet(state_local));
                    // br to loop: depth = (remaining blocks below us) + 1 (for loop)
                    let loop_depth = block_idx as u32;
                    f.instruction(&Instruction::Br(loop_depth));
                }
                Terminator::Branch {
                    cond,
                    then_label,
                    else_label,
                } => {
                    let then_idx = self.label_to_idx[then_label] as i32;
                    let else_idx = self.label_to_idx[else_label] as i32;

                    self.emit_truthiness_check(f, *cond);
                    f.instruction(&Instruction::If(BlockType::Empty));
                    f.instruction(&Instruction::I32Const(then_idx));
                    f.instruction(&Instruction::LocalSet(state_local));
                    f.instruction(&Instruction::Else);
                    f.instruction(&Instruction::I32Const(else_idx));
                    f.instruction(&Instruction::LocalSet(state_local));
                    f.instruction(&Instruction::End);

                    let loop_depth = block_idx as u32;
                    f.instruction(&Instruction::Br(loop_depth));
                }
                Terminator::Unreachable => {
                    f.instruction(&Instruction::Unreachable);
                }
                Terminator::Yield { .. } => {
                    f.instruction(&Instruction::Unreachable);
                }
            }
        }

        f.instruction(&Instruction::End); // loop
                                          // Unreachable — all paths through the loop use `return`
        f.instruction(&Instruction::Unreachable);
    }

    /// Emit a terminator that produces the function's return values.
    /// Used for single-block functions that don't need the loop dispatch.
    fn emit_terminator_return(&self, f: &mut Function, term: &Terminator) {
        match term {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            _ => {
                // Shouldn't happen for single-block functions
                f.instruction(&Instruction::Unreachable);
            }
        }
    }

    fn emit_truthiness_check(&self, f: &mut Function, cond: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_NIL as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::I32And);
    }

    fn emit_instr(&mut self, f: &mut Function, instr: &LirInstr) {
        match instr {
            LirInstr::Const { dst, value } => {
                self.emit_const(f, *dst, value);
            }
            LirInstr::ValueConst { dst, value } => {
                self.emit_value_const(f, *dst, *value);
            }
            LirInstr::BinOp { dst, op, lhs, rhs } => {
                self.emit_binop(f, *dst, *op, *lhs, *rhs);
            }
            LirInstr::Compare { dst, op, lhs, rhs } => {
                self.emit_compare(f, *dst, *op, *lhs, *rhs);
            }
            LirInstr::UnaryOp { dst, op, src } => {
                self.emit_unary(f, *dst, *op, *src);
            }
            LirInstr::IsNil { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_NIL),
            LirInstr::IsPair { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_CONS),
            LirInstr::IsArray { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_ARRAY),
            LirInstr::IsArrayMut { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_ARRAY_MUT),
            LirInstr::IsStruct { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_STRUCT),
            LirInstr::IsStructMut { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_STRUCT_MUT)
            }
            LirInstr::IsSet { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SET),
            LirInstr::IsSetMut { dst, src } => self.emit_tag_check(f, *dst, *src, TAG_SET_MUT),
            LirInstr::LoadLocal { dst, slot } => {
                if self.is_closure {
                    // Read from dedicated local-slot WASM locals
                    f.instruction(&Instruction::LocalGet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.local_slot_pay(*slot)));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    // Entry function: locals share the register space
                    let src = Reg(*slot as u32);
                    self.copy_reg(f, src, *dst);
                }
            }
            LirInstr::StoreLocal { slot, src } => {
                if self.is_closure {
                    // Write to dedicated local-slot WASM locals
                    f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_pay(*slot)));
                } else {
                    // Entry function: locals share the register space
                    let dst = Reg(*slot as u32);
                    self.copy_reg(f, *src, dst);
                }
            }
            LirInstr::LoadCapture { dst, index } => {
                // Load from closure env, auto-unwrap LBox.
                // env_ptr + index*16 → (tag, payload)
                let offset = (*index as u64) * 16;
                // Load tag
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                // Load payload
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                // Auto-unwrap LBox: if tag == TAG_LBOX, call rt_data_op(OP_LOAD_LBOX)
                f.instruction(&Instruction::LocalGet(self.tag_local(*dst)));
                f.instruction(&Instruction::I64Const(TAG_LBOX as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_data_op1(f, *dst, OP_LOAD_LBOX, *dst);
                f.instruction(&Instruction::End);
            }
            LirInstr::LoadCaptureRaw { dst, index } => {
                // Load from closure env WITHOUT unwrapping LBox.
                let offset = (*index as u64) * 16;
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
            }
            LirInstr::StoreCapture { index, src } => {
                // Store into a captured LBox cell.
                // First load the cell from env, then call rt_data_op(OP_STORE_LBOX).
                let offset = (*index as u64) * 16;
                // Load the cell (tag, payload) into temp — reuse src's locals
                // Actually, we need to read the cell handle from env, then
                // call store_lbox with (cell, new_value).
                // Write cell to args[0]
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I64Store(MemArg {
                    offset: 0,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::LocalGet(self.env_local()));
                f.instruction(&Instruction::I64Load(MemArg {
                    offset: offset + 8,
                    align: 3,
                    memory_index: 0,
                }));
                f.instruction(&Instruction::I64Store(MemArg {
                    offset: 8,
                    align: 3,
                    memory_index: 0,
                }));
                // Write new value to args[1]
                self.write_val_to_mem(f, *src, 1);
                // Call OP_STORE_LBOX
                f.instruction(&Instruction::I32Const(OP_STORE_LBOX));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(2));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                f.instruction(&Instruction::Drop); // signal (store_lbox can't fail)
                f.instruction(&Instruction::Drop); // payload
                f.instruction(&Instruction::Drop); // tag
            }
            LirInstr::Call { dst, func, args } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::TailCall { func, args } => {
                if !self.is_closure {
                    // Entry function: no env_ptr, use regular call + return
                    let dst = Reg(0);
                    self.emit_call(f, dst, *func, args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::Return);
                } else {
                    // Closure: write args, call rt_prepare_tail_call, dispatch
                    for (i, arg) in args.iter().enumerate() {
                        self.write_val_to_mem(f, *arg, i);
                    }
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(args.len() as i32));
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::RegionEnter | LirInstr::RegionExit => {}

            // --- Data operations via rt_data_op ---
            LirInstr::Cons { dst, head, tail } => {
                self.emit_data_op2(f, *dst, OP_CONS, *head, *tail);
            }
            LirInstr::Car { dst, pair } => {
                self.emit_data_op1(f, *dst, OP_CAR, *pair);
            }
            LirInstr::Cdr { dst, pair } => {
                self.emit_data_op1(f, *dst, OP_CDR, *pair);
            }
            LirInstr::CarDestructure { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CAR_DESTRUCTURE, *src);
            }
            LirInstr::CdrDestructure { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CDR_DESTRUCTURE, *src);
            }
            LirInstr::CarOrNil { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CAR_OR_NIL, *src);
            }
            LirInstr::CdrOrNil { dst, src } => {
                self.emit_data_op1(f, *dst, OP_CDR_OR_NIL, *src);
            }
            LirInstr::MakeArrayMut { dst, elements } => {
                self.emit_data_op_n(f, *dst, OP_MAKE_ARRAY, elements);
            }
            LirInstr::ArrayMutLen { dst, src } => {
                self.emit_data_op1(f, *dst, OP_ARRAY_LEN, *src);
            }
            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                // Pack index as an int value in the second arg slot
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_REF_DESTRUCTURE, *src, *index as i64);
            }
            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_SLICE_FROM, *src, *index as i64);
            }
            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                self.emit_data_op1_imm(f, *dst, OP_ARRAY_REF_OR_NIL, *src, *index as i64);
            }
            LirInstr::StructGetOrNil { dst, src, key } => {
                self.emit_struct_get(f, *dst, OP_STRUCT_GET_OR_NIL, *src, key);
            }
            LirInstr::StructGetDestructure { dst, src, key } => {
                self.emit_struct_get(f, *dst, OP_STRUCT_GET_DESTRUCTURE, *src, key);
            }
            LirInstr::ArrayMutExtend { dst, array, source } => {
                self.emit_data_op2(f, *dst, OP_ARRAY_EXTEND, *array, *source);
            }
            LirInstr::ArrayMutPush { dst, array, value } => {
                self.emit_data_op2(f, *dst, OP_ARRAY_PUSH, *array, *value);
            }
            LirInstr::MakeLBox { dst, value } => {
                self.emit_data_op1(f, *dst, OP_MAKE_LBOX, *value);
            }
            LirInstr::LoadLBox { dst, cell } => {
                self.emit_data_op1(f, *dst, OP_LOAD_LBOX, *cell);
            }
            LirInstr::StoreLBox { cell, value } => {
                // StoreLBox doesn't produce a result — use cell as dst (ignored)
                self.emit_data_op2(f, *cell, OP_STORE_LBOX, *cell, *value);
            }
            LirInstr::CallArrayMut { dst, func, args } => {
                // Call with args from an array register — write the func and array
                // to memory, host unpacks the array
                self.emit_call_array(f, *dst, *func, *args);
            }
            LirInstr::TailCallArrayMut { func, args } => {
                if !self.is_closure {
                    // Entry function: use regular call + return
                    let dst = Reg(0);
                    self.emit_call_array(f, dst, *func, *args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::Return);
                } else {
                    // Closure: write array arg, call rt_prepare_tail_call with nargs=-1
                    self.write_val_to_mem(f, *args, 1);
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(-1)); // nargs=-1: unpack array
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::Eval { dst, expr, env } => {
                // For now, stub eval as nil
                self.set_nil(f, *dst);
                let _ = (expr, env);
            }
            LirInstr::LoadResumeValue { dst } => {
                // Phase 2: stack switching
                self.set_nil(f, *dst);
            }
            LirInstr::MakeClosure {
                dst,
                func: nested,
                captures,
            } => {
                let table_idx = self
                    .closure_table_idx
                    .get(&(&**nested as *const LirFunction))
                    .copied()
                    .expect("MakeClosure: nested function not found in table");

                // Write captures to linear memory at ARGS_BASE
                for (i, cap) in captures.iter().enumerate() {
                    self.write_val_to_mem(f, *cap, i);
                }

                // Write metadata to linear memory after captures.
                // Metadata layout (starting at ARGS_BASE + captures.len()*16):
                //   [0]: num_captures (i64)
                //   [1]: num_params (i64)
                //   [2]: num_locals (i64)
                //   [3]: arity_kind (i64: 0=Exact, 1=AtLeast, 2=Range)
                //   [4]: arity_count (i64)
                //   [5]: lbox_params_mask (i64)
                //   [6]: lbox_locals_mask (i64)
                //   [7]: signal_bits (i64)
                let meta_base = ARGS_BASE + (captures.len() as i32) * 16;
                let meta_vals: [i64; 8] = [
                    nested.num_captures as i64,
                    nested.num_params as i64,
                    nested.num_locals as i64,
                    match nested.arity {
                        crate::value::types::Arity::Exact(_) => 0,
                        crate::value::types::Arity::AtLeast(_) => 1,
                        crate::value::types::Arity::Range(_, _) => 2,
                    },
                    match nested.arity {
                        crate::value::types::Arity::Exact(n) => n as i64,
                        crate::value::types::Arity::AtLeast(n) => n as i64,
                        crate::value::types::Arity::Range(min, _) => min as i64,
                    },
                    nested.lbox_params_mask as i64,
                    nested.lbox_locals_mask as i64,
                    nested.signal.bits.0 as i64,
                ];
                for (i, val) in meta_vals.iter().enumerate() {
                    f.instruction(&Instruction::I32Const(meta_base));
                    f.instruction(&Instruction::I64Const(*val));
                    f.instruction(&Instruction::I64Store(MemArg {
                        offset: (i * 8) as u64,
                        align: 3,
                        memory_index: 0,
                    }));
                }

                // Call rt_make_closure(table_idx, captures_ptr, metadata_ptr)
                f.instruction(&Instruction::I32Const(table_idx as i32));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(meta_base));
                f.instruction(&Instruction::Call(FN_RT_MAKE_CLOSURE));
                // Returns (tag, payload)
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
            }
            LirInstr::PushParamFrame { pairs } => {
                // Write (param_tag, param_payload, value_tag, value_payload)
                // for each pair to ARGS_BASE, then call rt_push_param.
                for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                    self.write_val_to_mem_offset(f, *param_reg, ARGS_BASE + (i as i32) * 32);
                    self.write_val_to_mem_offset(f, *val_reg, ARGS_BASE + (i as i32) * 32 + 16);
                }
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(pairs.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_PUSH_PARAM));
            }
            LirInstr::PopParamFrame => {
                f.instruction(&Instruction::Call(FN_RT_POP_PARAM));
            }
            LirInstr::CheckSignalBound { .. } => {
                // Compile-time validation — no-op at runtime.
            }
            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                // Write src struct as arg[0], exclude keys as arg[1..n].
                self.write_val_to_mem(f, *src, 0);
                for (i, key) in exclude_keys.iter().enumerate() {
                    let (tag, payload) = match key {
                        LirConst::Keyword(name) => {
                            (TAG_KEYWORD as i64, intern_keyword(name) as i64)
                        }
                        LirConst::Symbol(id) => (TAG_SYMBOL as i64, id.0 as i64),
                        _ => (TAG_NIL as i64, 0),
                    };
                    let offset = ((i + 1) * 16) as u64;
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I64Const(tag));
                    f.instruction(&Instruction::I64Store(MemArg {
                        offset,
                        align: 3,
                        memory_index: 0,
                    }));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I64Const(payload));
                    f.instruction(&Instruction::I64Store(MemArg {
                        offset: offset + 8,
                        align: 3,
                        memory_index: 0,
                    }));
                }
                f.instruction(&Instruction::I32Const(OP_STRUCT_REST));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(1 + exclude_keys.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                self.store_result_with_signal(f, *dst);
            }
        }
    }

    /// Emit a ValueConst. Immediates are inlined; heap values go through
    /// the constant pool and rt_load_const.
    fn emit_value_const(&mut self, f: &mut Function, dst: Reg, value: Value) {
        if value.tag < TAG_HEAP_START {
            // Immediate value — inline it
            f.instruction(&Instruction::I64Const(value.tag as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::I64Const(value.payload as i64));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        } else {
            // Heap value — add to constant pool, call rt_load_const
            let idx = self.const_pool.len() as i32;
            self.const_pool.push(value);
            f.instruction(&Instruction::I32Const(idx));
            f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
            // Stack: [tag: i64, payload: i64]
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        }
    }

    /// Emit a function call via rt_call host function.
    fn emit_call(&self, f: &mut Function, dst: Reg, func: Reg, args: &[Reg]) {
        // Write args to linear memory at ARGS_BASE
        for (i, arg) in args.iter().enumerate() {
            let offset = (i * 16) as u64;
            // Store tag
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.tag_local(*arg)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3, // 8-byte alignment
                memory_index: 0,
            }));
            // Store payload
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.pay_local(*arg)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }

        // Call rt_call(func_tag, func_payload, args_ptr, nargs, ctx)
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(args.len() as i32));
        f.instruction(&Instruction::I32Const(0)); // ctx = 0 for now
        f.instruction(&Instruction::Call(FN_RT_CALL));
        self.store_result_with_signal(f, dst);
    }

    /// Emit CallArrayMut: call a function with args from an array value.
    fn emit_call_array(&self, f: &mut Function, dst: Reg, func: Reg, args_array: Reg) {
        // Write func and args_array to memory as 2 args, then use rt_data_op
        // to unpack the array and call. Actually, simpler: just call rt_call
        // with func + array contents. But we don't know the array length at
        // compile time. Route through rt_call with a special nargs=-1 protocol
        // to mean "args_array is the second value, unpack it."
        //
        // For now: write [func, args_array] to memory, call rt_call with
        // nargs = -1 to signal "unpack array".
        self.write_val_to_mem(f, func, 0);
        self.write_val_to_mem(f, args_array, 1);
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE)); // points to args_array
        f.instruction(&Instruction::I32Const(-1)); // nargs = -1 means "unpack array at args_ptr"
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));
        self.store_result_with_signal(f, dst);
    }

    // --- Data operation helpers ---

    /// Store result from a host call that returns (tag, payload, signal).
    /// Signal is on top of stack. Checks signal and returns on error.
    fn store_result_with_signal(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit the dispatch logic after `rt_prepare_tail_call` returns.
    ///
    /// Stack on entry: `(env_ptr: i32, table_idx: i32, is_wasm: i32, tag: i64, payload: i64, signal: i32)`
    /// If is_wasm: `return_call_indirect` replaces the current frame.
    /// Otherwise: return (tag, payload) — the NativeFn/Parameter result.
    fn emit_tail_call_dispatch(&self, f: &mut Function) {
        // Temp locals for the 6 return values
        let tc_signal = self.signal_local;
        let tc_payload = self.pay_local(Reg(0));
        let tc_tag = self.tag_local(Reg(0));
        let tc_is_wasm = self.signal_local + 1;
        let tc_table_idx = self.signal_local + 2;
        let tc_env_ptr = self.signal_local + 3;

        // Pop results (WASM stack order: last return value on top)
        f.instruction(&Instruction::LocalSet(tc_signal)); // i32
        f.instruction(&Instruction::LocalSet(tc_payload)); // i64
        f.instruction(&Instruction::LocalSet(tc_tag)); // i64
        f.instruction(&Instruction::LocalSet(tc_is_wasm)); // i32
        f.instruction(&Instruction::LocalSet(tc_table_idx)); // i32
        f.instruction(&Instruction::LocalSet(tc_env_ptr)); // i32

        // Dispatch based on is_wasm
        f.instruction(&Instruction::LocalGet(tc_is_wasm));
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            // WASM closure: return_call_indirect replaces this frame
            f.instruction(&Instruction::LocalGet(tc_env_ptr));
            f.instruction(&Instruction::I32Const(0)); // args_ptr unused
            f.instruction(&Instruction::I32Const(0)); // nargs unused
            f.instruction(&Instruction::I32Const(0)); // ctx
            f.instruction(&Instruction::LocalGet(tc_table_idx));
            f.instruction(&Instruction::ReturnCallIndirect {
                type_index: 5,
                table_index: 0,
            });
        }
        f.instruction(&Instruction::Else);
        {
            // NativeFn/Parameter result: return (tag, payload)
            f.instruction(&Instruction::LocalGet(tc_tag));
            f.instruction(&Instruction::LocalGet(tc_payload));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);
    }

    /// 1-arg data op: write arg to memory, call rt_data_op, store result.
    fn emit_data_op1(&self, f: &mut Function, dst: Reg, op: i32, src: Reg) {
        self.write_val_to_mem(f, src, 0);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(1));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// 1-arg data op with an immediate second argument (e.g., array index).
    fn emit_data_op1_imm(&self, f: &mut Function, dst: Reg, op: i32, src: Reg, imm: i64) {
        self.write_val_to_mem(f, src, 0);
        // Write the immediate as a TAG_INT value in slot 1
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I64Const(TAG_INT as i64));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 16,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I64Const(imm));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 24,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// 2-arg data op.
    fn emit_data_op2(&self, f: &mut Function, dst: Reg, op: i32, a: Reg, b: Reg) {
        self.write_val_to_mem(f, a, 0);
        self.write_val_to_mem(f, b, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// N-arg data op (for MakeArrayMut etc).
    fn emit_data_op_n(&self, f: &mut Function, dst: Reg, op: i32, regs: &[Reg]) {
        for (i, reg) in regs.iter().enumerate() {
            self.write_val_to_mem(f, *reg, i);
        }
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(regs.len() as i32));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// Struct get with a constant key (keyword or symbol from LirConst).
    fn emit_struct_get(&mut self, f: &mut Function, dst: Reg, op: i32, src: Reg, key: &LirConst) {
        self.write_val_to_mem(f, src, 0);
        // Write key as Value in slot 1
        let (tag, payload) = match key {
            LirConst::Keyword(name) => (TAG_KEYWORD as i64, intern_keyword(name) as i64),
            LirConst::Symbol(id) => (TAG_SYMBOL as i64, id.0 as i64),
            _ => (TAG_NIL as i64, 0),
        };
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I64Const(tag));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 16,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I64Const(payload));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 24,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// Write a register's value to linear memory at ARGS_BASE + slot*16.
    fn write_val_to_mem(&self, f: &mut Function, reg: Reg, slot: usize) {
        self.write_val_to_mem_offset(f, reg, ARGS_BASE + (slot as i32) * 16);
    }

    /// Write a register's value to linear memory at an absolute byte offset.
    fn write_val_to_mem_offset(&self, f: &mut Function, reg: Reg, base: i32) {
        f.instruction(&Instruction::I32Const(base));
        f.instruction(&Instruction::LocalGet(self.tag_local(reg)));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(base));
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 8,
            align: 3,
            memory_index: 0,
        }));
    }

    fn emit_const(&mut self, f: &mut Function, dst: Reg, value: &LirConst) {
        match value {
            LirConst::String(s) => {
                // String is a heap value — add to constant pool and load via host
                let str_val = Value::string(s.clone());
                let idx = self.const_pool.len() as i32;
                self.const_pool.push(str_val);
                f.instruction(&Instruction::I32Const(idx));
                f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            }
            _ => {
                let (tag, payload) = match value {
                    LirConst::Nil => (TAG_NIL as i64, 0i64),
                    LirConst::EmptyList => (TAG_EMPTY_LIST as i64, 0),
                    LirConst::Bool(true) => (TAG_TRUE as i64, 0),
                    LirConst::Bool(false) => (TAG_FALSE as i64, 0),
                    LirConst::Int(n) => (TAG_INT as i64, *n),
                    LirConst::Float(x) => (TAG_FLOAT as i64, x.to_bits() as i64),
                    LirConst::Symbol(id) => (TAG_SYMBOL as i64, id.0 as i64),
                    LirConst::Keyword(name) => (TAG_KEYWORD as i64, intern_keyword(name) as i64),
                    LirConst::String(_) => unreachable!(),
                };
                f.instruction(&Instruction::I64Const(tag));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(payload));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
    }

    fn emit_binop(&self, f: &mut Function, dst: Reg, op: BinOp, lhs: Reg, rhs: Reg) {
        f.instruction(&Instruction::I64Const(TAG_INT as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(lhs)));
        f.instruction(&Instruction::LocalGet(self.pay_local(rhs)));
        match op {
            BinOp::Add => f.instruction(&Instruction::I64Add),
            BinOp::Sub => f.instruction(&Instruction::I64Sub),
            BinOp::Mul => f.instruction(&Instruction::I64Mul),
            BinOp::Div => f.instruction(&Instruction::I64DivS),
            BinOp::Rem => f.instruction(&Instruction::I64RemS),
            BinOp::BitAnd => f.instruction(&Instruction::I64And),
            BinOp::BitOr => f.instruction(&Instruction::I64Or),
            BinOp::BitXor => f.instruction(&Instruction::I64Xor),
            BinOp::Shl => f.instruction(&Instruction::I64Shl),
            BinOp::Shr => f.instruction(&Instruction::I64ShrS),
        };
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }

    fn emit_compare(&self, f: &mut Function, dst: Reg, op: CmpOp, lhs: Reg, rhs: Reg) {
        f.instruction(&Instruction::LocalGet(self.pay_local(lhs)));
        f.instruction(&Instruction::LocalGet(self.pay_local(rhs)));
        match op {
            CmpOp::Eq => f.instruction(&Instruction::I64Eq),
            CmpOp::Ne => f.instruction(&Instruction::I64Ne),
            CmpOp::Lt => f.instruction(&Instruction::I64LtS),
            CmpOp::Le => f.instruction(&Instruction::I64LeS),
            CmpOp::Gt => f.instruction(&Instruction::I64GtS),
            CmpOp::Ge => f.instruction(&Instruction::I64GeS),
        };
        self.emit_bool_from_i32(f, dst);
    }

    fn emit_unary(&self, f: &mut Function, dst: Reg, op: UnaryOp, src: Reg) {
        match op {
            UnaryOp::Neg => {
                f.instruction(&Instruction::I64Const(TAG_INT as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::LocalGet(self.pay_local(src)));
                f.instruction(&Instruction::I64Sub);
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
            UnaryOp::Not => {
                f.instruction(&Instruction::LocalGet(self.tag_local(src)));
                f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::LocalGet(self.tag_local(src)));
                f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                f.instruction(&Instruction::I64Eq);
                f.instruction(&Instruction::I32Or);
                self.emit_bool_from_i32(f, dst);
            }
            UnaryOp::BitNot => {
                f.instruction(&Instruction::I64Const(TAG_INT as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(-1));
                f.instruction(&Instruction::LocalGet(self.pay_local(src)));
                f.instruction(&Instruction::I64Xor);
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
    }

    fn emit_tag_check(&self, f: &mut Function, dst: Reg, src: Reg, expected_tag: u64) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::I64Const(expected_tag as i64));
        f.instruction(&Instruction::I64Eq);
        self.emit_bool_from_i32(f, dst);
    }

    fn emit_bool_from_i32(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I64Const(TAG_TRUE as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::Else);
        f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::End);
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }

    fn copy_reg(&self, f: &mut Function, src: Reg, dst: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(src)));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }

    fn set_nil(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::I64Const(TAG_NIL as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }

    /// Emit a closure function body.
    ///
    /// Closure WASM type: `(env_ptr: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, pay: i64)`
    ///
    /// WASM local layout:
    /// - 0: env_ptr (param)
    /// - 1: args_ptr (param) — unused
    /// - 2: nargs (param) — unused
    /// - 3: ctx (param)
    /// - 4..4+N: register tags (computation intermediates)
    /// - 4+N..4+2N: register payloads
    /// - 4+2N..4+2N+M: local slot tags (non-LBox let-bound variables)
    /// - 4+2N+M..4+2N+2M: local slot payloads
    /// - 4+2N+2M: signal scratch (i32)
    /// - 4+2N+2M+1..+3: tail call temps (i32)
    ///
    /// Three address spaces:
    /// - Env (linear memory at env_ptr): captures, params, LBox locals
    /// - Local slots (WASM locals): non-LBox let-bound variables
    /// - Registers (WASM locals): computation intermediates
    fn emit_closure_function(&mut self, func: &LirFunction) -> Function {
        self.label_to_idx.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        self.num_regs = func.num_regs;
        self.local_offset = 4;
        self.is_closure = true;
        self.num_stack_locals = func.num_locals as u32;

        // Layout: params(4) + reg_tags(N) + reg_pays(N) + local_tags(M) + local_pays(M) + i32s(4)
        let n = func.num_regs;
        let m = self.num_stack_locals;
        self.signal_local = 4 + 2 * n + 2 * m;

        let mut f = Function::new([
            (n, ValType::I64), // register tags
            (n, ValType::I64), // register payloads
            (m, ValType::I64), // local slot tags
            (m, ValType::I64), // local slot payloads
            (4, ValType::I32), // signal_scratch + 3 tail call temps
        ]);

        self.emit_cfg(&mut f, func);

        f.instruction(&Instruction::End);
        f
    }

    /// WASM local index for the env pointer.
    /// Entry function: local at 2*num_regs (declared as additional local).
    /// Closure function: param 0 (WASM local 0).
    fn env_local(&self) -> u32 {
        if self.local_offset == 0 {
            // Entry function: env_ptr is the last declared local
            self.num_regs * 2
        } else {
            // Closure function: env_ptr is param 0
            0
        }
    }

    fn tag_local(&self, reg: Reg) -> u32 {
        self.local_offset + reg.0
    }

    fn pay_local(&self, reg: Reg) -> u32 {
        self.local_offset + reg.0 + self.num_regs
    }

    /// WASM local index for a stack-local variable's tag.
    /// Local slots live after the register bank: offset + 2*num_regs + slot.
    fn local_slot_tag(&self, slot: u16) -> u32 {
        self.local_offset + 2 * self.num_regs + slot as u32
    }

    /// WASM local index for a stack-local variable's payload.
    fn local_slot_pay(&self, slot: u16) -> u32 {
        self.local_offset + 2 * self.num_regs + self.num_stack_locals + slot as u32
    }
}

/// Recursively collect all nested LirFunctions from MakeClosure instructions.
fn collect_nested_functions<'a>(func: &'a LirFunction, out: &mut Vec<&'a LirFunction>) {
    for block in &func.blocks {
        for spanned in &block.instructions {
            if let LirInstr::MakeClosure { func: nested, .. } = &spanned.instr {
                out.push(nested);
                // Recurse into the nested function's own closures
                collect_nested_functions(nested, out);
            }
        }
    }
}
