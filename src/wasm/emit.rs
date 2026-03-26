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

// First non-imported function index
const FN_ENTRY: u32 = 3;

// Linear memory layout
const ARGS_BASE: i32 = 256; // Args buffer starts at byte 256

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
        module.section(&types);

        // Import section
        let mut imports = ImportSection::new();
        imports.import("elle", "call_primitive", EntityType::Function(1)); // FN_CALL_PRIMITIVE
        imports.import("elle", "rt_call", EntityType::Function(2)); // FN_RT_CALL
        imports.import("elle", "rt_load_const", EntityType::Function(3)); // FN_RT_LOAD_CONST
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
            (func.num_regs, ValType::I64), // tags
            (func.num_regs, ValType::I64), // payloads
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
            LirInstr::ArrayMutLen { dst, src } => {
                // Dispatch to host — length is a primitive operation on heap objects.
                // For now, stub as nil.
                self.set_nil(f, *dst);
                let _ = src;
            }
            _ => {
                // Unimplemented — no-op stub.
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
