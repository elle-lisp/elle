//! LIR → WASM emission.
//!
//! Converts a `LirFunction` into WASM module bytes using `wasm-encoder`.
//! Each LIR register maps to two WASM locals (tag: i64, payload: i64).
//! Immediate values (int, float, nil, bool) are constructed in WASM.
//! Heap operations go through host function calls.
//!
//! Control flow: LIR basic blocks are emitted recursively. Branch terminators
//! become WASM `if/else/end`. Jump terminators inline the target block.
//! Both arms of a Branch that Jump to the same target produce a merge block
//! emitted after the `end`.

use crate::lir::{BinOp, CmpOp, Label, LirConst, LirFunction, LirInstr, Reg, Terminator, UnaryOp};
use crate::value::keyword::intern_keyword;
use crate::value::repr::*;
use std::collections::{HashMap, HashSet};
use wasm_encoder::*;

/// Emit a WASM module from a top-level LirFunction.
pub fn emit_module(func: &LirFunction) -> Vec<u8> {
    let mut emitter = WasmEmitter::new();
    emitter.emit_module(func)
}

struct WasmEmitter {
    label_to_idx: HashMap<Label, usize>,
    num_regs: u32,
    /// Blocks already emitted (to avoid double-emission of merge blocks).
    emitted: HashSet<Label>,
}

impl WasmEmitter {
    fn new() -> Self {
        WasmEmitter {
            label_to_idx: HashMap::new(),
            num_regs: 0,
            emitted: HashSet::new(),
        }
    }

    fn emit_module(&mut self, func: &LirFunction) -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function([], [ValType::I64, ValType::I64]);
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import("elle", "call_primitive", EntityType::Function(1));
        module.section(&imports);

        let mut functions = FunctionSection::new();
        functions.function(0);
        module.section(&functions);

        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memories);

        let mut exports = ExportSection::new();
        exports.export("__elle_entry", ExportKind::Func, 1);
        exports.export("__elle_memory", ExportKind::Memory, 0);
        module.section(&exports);

        let body = self.emit_function(func);
        let mut code = CodeSection::new();
        code.function(&body);
        module.section(&code);

        module.finish()
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

    /// Recursively emit a block and its successors.
    fn emit_block_tree(&mut self, f: &mut Function, label: Label, func: &LirFunction) {
        if self.emitted.contains(&label) {
            return;
        }
        self.emitted.insert(label);

        let idx = self.label_to_idx[&label];
        let block = &func.blocks[idx];

        // Emit instructions
        for spanned in &block.instructions {
            self.emit_instr(f, &spanned.instr);
        }

        // Emit terminator and recurse into successors
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
                // Inline the target block
                self.emit_block_tree(f, *target, func);
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                // Emit truthiness check: !(tag == TAG_NIL || tag == TAG_FALSE)
                f.instruction(&Instruction::LocalGet(self.tag_local(*cond)));
                f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                f.instruction(&Instruction::I64Ne);
                f.instruction(&Instruction::LocalGet(self.tag_local(*cond)));
                f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                f.instruction(&Instruction::I64Ne);
                f.instruction(&Instruction::I32And);

                // Find the merge point by looking for a common Jump target
                // across the two branch trees.
                let merge_label =
                    find_merge_label(func, *then_label, *else_label, &self.label_to_idx);

                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_branch_arm(f, *then_label, merge_label, func);
                f.instruction(&Instruction::Else);
                self.emit_branch_arm(f, *else_label, merge_label, func);
                f.instruction(&Instruction::End);

                // Emit merge block if both branches converge
                if let Some(merge) = merge_label {
                    self.emit_block_tree(f, merge, func);
                }
            }
            Terminator::Yield { .. } => {
                // Phase 2: stack switching
                f.instruction(&Instruction::Unreachable);
            }
        }
    }

    /// Emit the body of a branch arm, stopping before the merge label.
    ///
    /// Recursively emits the block tree rooted at `label`, but does NOT
    /// emit the merge block (that's handled by the caller after `end`).
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
            Terminator::Jump(target) if merge == Some(*target) => {
                // This block jumps to the merge point — stop here.
            }
            Terminator::Jump(target) => {
                // Jump to a block that isn't the merge — continue emitting.
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
                // Nested branch inside a branch arm. Find the sub-merge.
                let sub_merge =
                    find_merge_label(func, *then_label, *else_label, &self.label_to_idx);

                f.instruction(&Instruction::LocalGet(self.tag_local(*cond)));
                f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                f.instruction(&Instruction::I64Ne);
                f.instruction(&Instruction::LocalGet(self.tag_local(*cond)));
                f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                f.instruction(&Instruction::I64Ne);
                f.instruction(&Instruction::I32And);

                f.instruction(&Instruction::If(BlockType::Empty));
                self.emit_branch_arm(f, *then_label, sub_merge, func);
                f.instruction(&Instruction::Else);
                self.emit_branch_arm(f, *else_label, sub_merge, func);
                f.instruction(&Instruction::End);

                // Emit the sub-merge block, stopping at the outer merge
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

    fn emit_instr(&self, f: &mut Function, instr: &LirInstr) {
        match instr {
            LirInstr::Const { dst, value } => {
                self.emit_const(f, *dst, value);
            }
            LirInstr::ValueConst { dst, value } => {
                f.instruction(&Instruction::I64Const(value.tag as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::I64Const(value.payload as i64));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
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
            LirInstr::IsNil { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_NIL);
            }
            LirInstr::IsPair { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_CONS);
            }
            LirInstr::IsArray { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_ARRAY);
            }
            LirInstr::IsArrayMut { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_ARRAY_MUT);
            }
            LirInstr::IsStruct { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_STRUCT);
            }
            LirInstr::IsStructMut { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_STRUCT_MUT);
            }
            LirInstr::IsSet { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_SET);
            }
            LirInstr::IsSetMut { dst, src } => {
                self.emit_tag_check(f, *dst, *src, TAG_SET_MUT);
            }
            LirInstr::LoadLocal { dst, slot } => {
                let src = Reg(*slot as u32);
                f.instruction(&Instruction::LocalGet(self.tag_local(src)));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                f.instruction(&Instruction::LocalGet(self.pay_local(src)));
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
            }
            LirInstr::StoreLocal { slot, src } => {
                let dst = Reg(*slot as u32);
                f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*src)));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
            LirInstr::RegionEnter | LirInstr::RegionExit => {
                // No-op for now. Heap objects are host-managed.
            }
            _ => {
                // Unimplemented — no-op stub.
            }
        }
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
                // not: false/nil → true, else → false
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

    /// Emit a type tag check: dst = (src.tag == expected_tag)
    fn emit_tag_check(&self, f: &mut Function, dst: Reg, src: Reg, expected_tag: u64) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::I64Const(expected_tag as i64));
        f.instruction(&Instruction::I64Eq);
        self.emit_bool_from_i32(f, dst);
    }

    /// Convert an i32 (0 or 1) on the WASM stack to an Elle boolean Value.
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

    fn tag_local(&self, reg: Reg) -> u32 {
        reg.0
    }

    fn pay_local(&self, reg: Reg) -> u32 {
        reg.0 + self.num_regs
    }
}

/// Walk a branch arm's block chain to find where it ultimately Jumps.
/// Follows Jump chains through intermediate blocks.
fn ultimate_jump_target(
    func: &LirFunction,
    start: Label,
    label_to_idx: &HashMap<Label, usize>,
) -> Option<Label> {
    let mut current = start;
    for _ in 0..20 {
        // depth limit to prevent infinite loops
        let idx = label_to_idx[&current];
        match &func.blocks[idx].terminator.terminator {
            Terminator::Jump(target) => {
                // If this block has instructions, its jump target is the answer
                // (it's doing work then merging)
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
                // Nested branch — find where IT merges, then continue
                if let Some(merge) = find_merge_label(func, *then_label, *else_label, label_to_idx)
                {
                    // The nested branch merges at `merge`. Follow that chain.
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

/// Find the common merge label for two branch arms.
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
