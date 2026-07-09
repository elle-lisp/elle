//! Linear-memory marshalling and constant materialization helpers.
//!
//! Register-to-memory stores at `ARGS_BASE`, truthiness checks, and the const
//! emitters that either inline immediate tagged values or route heap values
//! through the module const pool (`rt_load_const`).

use super::*;

impl WasmEmitter {
    /// Emit a ValueConst (immediate inline or heap via const pool).
    pub(in crate::wasm) fn emit_value_const(&mut self, f: &mut Function, dst: Reg, value: Value) {
        if value.tag < TAG_HEAP_START {
            f.instruction(&Instruction::I64Const(value.tag as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::I64Const(value.payload as i64));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        } else {
            let idx = self.const_pool.len() as i32;
            self.const_pool.push(value);
            f.instruction(&Instruction::I32Const(idx));
            f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        }
    }

    /// Write a register to linear memory at ARGS_BASE + slot*16.
    pub(in crate::wasm) fn write_val_to_mem(&self, f: &mut Function, reg: Reg, slot: usize) {
        self.write_val_to_mem_offset(f, reg, ARGS_BASE + (slot as i32) * 16);
    }

    /// Write a register to linear memory at an absolute offset.
    pub(in crate::wasm) fn write_val_to_mem_offset(&self, f: &mut Function, reg: Reg, base: i32) {
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

    /// Emit truthiness check: pushes i32 (0=falsy, 1=truthy).
    pub(in crate::wasm) fn emit_truthiness_check(&self, f: &mut Function, cond: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_FALSE as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::LocalGet(self.tag_local(cond)));
        f.instruction(&Instruction::I64Const(TAG_NIL as i64));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::I32And);
    }

    /// Add value to const pool and emit rt_load_const into dst.
    pub(in crate::wasm) fn emit_const_pool_load(
        &mut self,
        f: &mut Function,
        dst: Reg,
        value: Value,
    ) {
        let idx = self.const_pool.len() as i32;
        self.const_pool.push(value);
        f.instruction(&Instruction::I32Const(idx));
        f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    pub(in crate::wasm) fn emit_const(&mut self, f: &mut Function, dst: Reg, value: &LirConst) {
        match value {
            LirConst::String(s) => {
                // A compile-time const-pool template string, built on the driving
                // instance's heap (threaded into the emitter); the wasm runtime
                // re-materializes it from the pool on `rt_load_const`. It lives for
                // the module's lifetime, held by the module's const pool.
                let heap_ptr = self.heap_ptr;
                let region = unsafe { (*heap_ptr).new_runtime_region() };
                let sval =
                    crate::value::build::string(unsafe { &mut *heap_ptr }, s.clone(), region);
                self.emit_const_pool_load(f, dst, sval);
            }
            LirConst::Symbol(id) => {
                self.emit_const_pool_load(f, dst, Value::symbol(id.0));
            }
            LirConst::Keyword(name) => {
                self.emit_const_pool_load(f, dst, Value::keyword(name));
            }
            _ => {
                let (tag, payload) = match value {
                    LirConst::Nil => (TAG_NIL as i64, 0i64),
                    LirConst::EmptyList => (TAG_EMPTY_LIST as i64, 0),
                    LirConst::Bool(true) => (TAG_TRUE as i64, 0),
                    LirConst::Bool(false) => (TAG_FALSE as i64, 0),
                    LirConst::Int(n) => (TAG_INT as i64, *n),
                    LirConst::Float(x) => (TAG_FLOAT as i64, x.to_bits() as i64),
                    LirConst::Symbol(_)
                    | LirConst::Keyword(_)
                    | LirConst::String(_)
                    | LirConst::ClosureRef(_)
                    | LirConst::ValueRef(_) => {
                        unreachable!()
                    }
                };
                f.instruction(&Instruction::I64Const(tag));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(payload));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
    }

    pub(in crate::wasm) fn copy_reg(&self, f: &mut Function, src: Reg, dst: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(src)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(src)));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }
}
