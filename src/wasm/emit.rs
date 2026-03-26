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
use std::collections::{HashMap, HashSet};
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

// First non-imported function index
const FN_ENTRY: u32 = 4;

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

struct WasmEmitter {
    label_to_idx: HashMap<Label, usize>,
    num_regs: u32,
    emitted: HashSet<Label>,
    /// Heap constants collected during emission.
    const_pool: Vec<Value>,
}

impl WasmEmitter {
    fn new() -> Self {
        WasmEmitter {
            label_to_idx: HashMap::new(),
            num_regs: 0,
            emitted: HashSet::new(),
            const_pool: Vec::new(),
        }
    }

    fn emit_module(&mut self, func: &LirFunction) -> EmitResult {
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
        module.section(&types);

        // Import section
        let mut imports = ImportSection::new();
        imports.import("elle", "call_primitive", EntityType::Function(1));
        imports.import("elle", "rt_call", EntityType::Function(2));
        imports.import("elle", "rt_load_const", EntityType::Function(3));
        imports.import("elle", "rt_data_op", EntityType::Function(4));
        module.section(&imports);

        // Function section
        let mut functions = FunctionSection::new();
        functions.function(0); // entry function: type 0
        module.section(&functions);

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
        module.section(&exports);

        // Code section
        let body = self.emit_function(func);
        let mut code = CodeSection::new();
        code.function(&body);
        module.section(&code);

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
        }
    }

    fn emit_function(&mut self, func: &LirFunction) -> Function {
        self.label_to_idx.clear();
        self.emitted.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        self.num_regs = func.num_regs;

        let mut f = Function::new([
            (func.num_regs, ValType::I64), // tags: locals [0, num_regs)
            (func.num_regs, ValType::I64), // payloads: locals [num_regs, 2*num_regs)
            (1, ValType::I32),             // env_ptr: local 2*num_regs
        ]);

        self.emit_block_tree(&mut f, func.entry, func);

        f.instruction(&Instruction::End);
        f
    }

    fn emit_block_tree(&mut self, f: &mut Function, label: Label, func: &LirFunction) {
        if self.emitted.contains(&label) {
            return;
        }
        self.emitted.insert(label);

        let idx = self.label_to_idx[&label];
        let block = &func.blocks[idx];

        for spanned in &block.instructions {
            self.emit_instr(f, &spanned.instr);
        }

        match &block.terminator.terminator {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::Return);
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            Terminator::Jump(target) => {
                self.emit_block_tree(f, *target, func);
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                self.emit_branch(f, *cond, *then_label, *else_label, func);
            }
            Terminator::Yield { .. } => {
                f.instruction(&Instruction::Unreachable);
            }
        }
    }

    fn emit_branch(
        &mut self,
        f: &mut Function,
        cond: Reg,
        then_label: Label,
        else_label: Label,
        func: &LirFunction,
    ) {
        // Truthiness: !(tag == TAG_NIL || tag == TAG_FALSE)
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_NIL as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::I32And);

        let merge = find_merge_label(func, then_label, else_label, &self.label_to_idx);

        f.instruction(&Instruction::If(BlockType::Empty));
        self.emit_branch_arm(f, then_label, merge, func);
        f.instruction(&Instruction::Else);
        self.emit_branch_arm(f, else_label, merge, func);
        f.instruction(&Instruction::End);

        if let Some(merge) = merge {
            self.emit_block_tree(f, merge, func);
        }
    }

    fn emit_branch_arm(
        &mut self,
        f: &mut Function,
        label: Label,
        merge: Option<Label>,
        func: &LirFunction,
    ) {
        if self.emitted.contains(&label) {
            return;
        }
        self.emitted.insert(label);

        let idx = self.label_to_idx[&label];
        let block = &func.blocks[idx];

        for spanned in &block.instructions {
            self.emit_instr(f, &spanned.instr);
        }

        match &block.terminator.terminator {
            Terminator::Jump(target) if merge == Some(*target) => {}
            Terminator::Jump(target) => {
                self.emit_branch_arm(f, *target, merge, func);
            }
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::Return);
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let sub_merge =
                    find_merge_label(func, *then_label, *else_label, &self.label_to_idx);

                self.emit_truthiness_check(f, *cond);
                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_branch_arm(f, *then_label, sub_merge, func);
                f.instruction(&Instruction::Else);
                self.emit_branch_arm(f, *else_label, sub_merge, func);
                f.instruction(&Instruction::End);

                if let Some(sm) = sub_merge {
                    self.emit_branch_arm(f, sm, merge, func);
                }
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            Terminator::Yield { .. } => {
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
                let src = Reg(*slot as u32);
                self.copy_reg(f, src, *dst);
            }
            LirInstr::StoreLocal { slot, src } => {
                let dst = Reg(*slot as u32);
                self.copy_reg(f, *src, dst);
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
                f.instruction(&Instruction::Drop); // signal
                f.instruction(&Instruction::Drop); // payload
                f.instruction(&Instruction::Drop); // tag
            }
            LirInstr::Call { dst, func, args } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::TailCall { func, args } => {
                // For now, emit as regular call + return.
                // TODO: use return_call_indirect for WASM tail calls
                let dst = Reg(0); // reuse reg 0 as temp
                self.emit_call(f, dst, *func, args);
                f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                f.instruction(&Instruction::Return);
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
                let dst = Reg(0);
                self.emit_call_array(f, dst, *func, *args);
                f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                f.instruction(&Instruction::Return);
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
            LirInstr::PushParamFrame { .. }
            | LirInstr::PopParamFrame
            | LirInstr::CheckSignalBound { .. }
            | LirInstr::StructRest { .. } => {
                // TODO: implement via host functions
            }
            _ => {
                // Remaining instructions — no-op stub
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
        // Stack: [tag: i64, payload: i64, signal: i32]
        f.instruction(&Instruction::Drop); // drop signal_bits for now
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
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
        f.instruction(&Instruction::Drop);
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    // --- Data operation helpers ---

    /// 1-arg data op: write arg to memory, call rt_data_op, store result.
    fn emit_data_op1(&self, f: &mut Function, dst: Reg, op: i32, src: Reg) {
        self.write_val_to_mem(f, src, 0);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(1));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        f.instruction(&Instruction::Drop); // signal
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
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
        f.instruction(&Instruction::Drop);
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    /// 2-arg data op.
    fn emit_data_op2(&self, f: &mut Function, dst: Reg, op: i32, a: Reg, b: Reg) {
        self.write_val_to_mem(f, a, 0);
        self.write_val_to_mem(f, b, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        f.instruction(&Instruction::Drop);
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
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
        f.instruction(&Instruction::Drop);
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
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
        f.instruction(&Instruction::Drop);
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    /// Write a register's value to linear memory at ARGS_BASE + slot*16.
    fn write_val_to_mem(&self, f: &mut Function, reg: Reg, slot: usize) {
        let offset = (slot * 16) as u64;
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::LocalGet(self.tag_local(reg)));
        f.instruction(&Instruction::I64Store(MemArg {
            offset,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: offset + 8,
            align: 3,
            memory_index: 0,
        }));
    }

    fn emit_const(&self, f: &mut Function, dst: Reg, value: &LirConst) {
        let (tag, payload) = match value {
            LirConst::Nil => (TAG_NIL as i64, 0i64),
            LirConst::EmptyList => (TAG_EMPTY_LIST as i64, 0),
            LirConst::Bool(true) => (TAG_TRUE as i64, 0),
            LirConst::Bool(false) => (TAG_FALSE as i64, 0),
            LirConst::Int(n) => (TAG_INT as i64, *n),
            LirConst::Float(x) => (TAG_FLOAT as i64, x.to_bits() as i64),
            LirConst::Symbol(id) => (TAG_SYMBOL as i64, id.0 as i64),
            LirConst::Keyword(name) => (TAG_KEYWORD as i64, intern_keyword(name) as i64),
            LirConst::String(_) => (TAG_NIL as i64, 0), // stub
        };
        f.instruction(&Instruction::I64Const(tag));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::I64Const(payload));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
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

    /// WASM local index for the env pointer (i32).
    fn env_local(&self) -> u32 {
        self.num_regs * 2
    }

    fn tag_local(&self, reg: Reg) -> u32 {
        reg.0
    }

    fn pay_local(&self, reg: Reg) -> u32 {
        reg.0 + self.num_regs
    }
}

fn ultimate_jump_target(
    func: &LirFunction,
    start: Label,
    label_to_idx: &HashMap<Label, usize>,
) -> Option<Label> {
    let mut current = start;
    for _ in 0..20 {
        let idx = label_to_idx[&current];
        match &func.blocks[idx].terminator.terminator {
            Terminator::Jump(target) => {
                if !func.blocks[idx].instructions.is_empty() || current == start {
                    return Some(*target);
                }
                current = *target;
            }
            Terminator::Branch {
                then_label,
                else_label,
                ..
            } => {
                if let Some(merge) = find_merge_label(func, *then_label, *else_label, label_to_idx)
                {
                    let merge_idx = label_to_idx[&merge];
                    match &func.blocks[merge_idx].terminator.terminator {
                        Terminator::Jump(target) => return Some(*target),
                        _ => return None,
                    }
                }
                return None;
            }
            _ => return None,
        }
    }
    None
}

fn find_merge_label(
    func: &LirFunction,
    then_label: Label,
    else_label: Label,
    label_to_idx: &HashMap<Label, usize>,
) -> Option<Label> {
    let then_target = ultimate_jump_target(func, then_label, label_to_idx);
    let else_target = ultimate_jump_target(func, else_label, label_to_idx);
    if then_target.is_some() && then_target == else_target {
        then_target
    } else {
        None
    }
}
