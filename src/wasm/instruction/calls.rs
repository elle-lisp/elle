//! Closure construction, calls, tail calls, and result handling.
//!
//! These emitters marshal register values into linear memory at `ARGS_BASE`,
//! invoke the runtime dispatch functions (`rt_call`, `rt_make_closure`), and
//! unpack the words they return. `rt_call` returns four —
//! `(tag, payload, signal, suspended)` — while `rt_data_op` and `call_primitive`
//! return three; the two shapes have separate store helpers below.

use super::*;

impl WasmEmitter {
    /// Emit MakeClosure: write captures + metadata to memory, call rt_make_closure.
    pub(super) fn emit_make_closure(
        &mut self,
        f: &mut Function,
        dst: Reg,
        closure_id: crate::lir::ClosureId,
        captures: &[Reg],
    ) {
        let table_idx = self
            .closure_id_to_table_idx
            .get(&closure_id)
            .copied()
            .expect("MakeClosure: ClosureId not found in table map");
        let nested = &self
            .module_closures
            .as_ref()
            .expect("MakeClosure: no module_closures context")[closure_id.0 as usize];

        for (i, cap) in captures.iter().enumerate() {
            self.write_val_to_mem(f, *cap, i);
        }

        let meta_base = ARGS_BASE + (captures.len() as i32) * 16;
        // `capture_locals_mask` is unbounded (`CaptureMask`), so slot 6 carries
        // the WORD COUNT and the words are appended after the 8 fixed slots
        // (read symmetrically in `rt_make_closure`, src/wasm/linker/create.rs).
        // The common case (no captured locals) is 0 words.
        let locals_mask_words = nested.capture_locals_mask.words();
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
            nested.capture_params_mask as i64,
            locals_mask_words.len() as i64,
            nested.signal.bits.raw() as i64,
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
        // Append the locals-mask words after the 8 fixed slots.
        for (j, w) in locals_mask_words.iter().enumerate() {
            f.instruction(&Instruction::I32Const(meta_base));
            f.instruction(&Instruction::I64Const(*w as i64));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: ((8 + j) * 8) as u64,
                align: 3,
                memory_index: 0,
            }));
        }

        f.instruction(&Instruction::I32Const(table_idx as i32));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(meta_base));
        f.instruction(&Instruction::Call(FN_RT_MAKE_CLOSURE));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    /// Emit a function call via rt_call.
    pub(in crate::wasm) fn emit_call(&self, f: &mut Function, dst: Reg, func: Reg, args: &[Reg]) {
        for (i, arg) in args.iter().enumerate() {
            let offset = (i * 16) as u64;
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.tag_local(*arg)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.pay_local(*arg)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }

        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(args.len() as i32));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));
        self.store_call_result(f, dst);
    }

    /// Emit CallArrayMut via rt_call with nargs=-1 protocol.
    pub(in crate::wasm) fn emit_call_array(
        &self,
        f: &mut Function,
        dst: Reg,
        func: Reg,
        args_array: Reg,
    ) {
        self.write_val_to_mem(f, func, 0);
        self.write_val_to_mem(f, args_array, 1);
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(-1));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));
        self.store_call_result(f, dst);
    }

    /// Store `rt_call`'s FOUR-word result in a NON-suspending function.
    ///
    /// This function has no continuation frame to capture, so a callee that
    /// parked is handled the same way as one that raised: the signal goes to
    /// `SIGNAL_SLOT` and the function returns with `status = 0`. The caller's
    /// `handle_wasm_result` reads the slot and classifies it there — that is the
    /// yield-through path, and it is why `suspended` is popped and dropped here
    /// rather than branched on.
    ///
    /// Only `rt_call` returns four words. `rt_data_op` and `call_primitive`
    /// return three and use [`store_result_with_signal`] directly.
    pub(in crate::wasm) fn store_call_result(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::LocalSet(self.suspended_local()));
        self.store_result_with_signal(f, dst);
    }

    /// Store a THREE-word `(tag, payload, signal)` result, early-returning on
    /// any signal. Shared by `rt_data_op` and `call_primitive`.
    pub(in crate::wasm) fn store_result_with_signal(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::I32Const(SIGNAL_SLOT));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I64Store(MemArg {
            offset: 0,
            align: 3,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::I64Ne);
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit tail call dispatch after rt_prepare_tail_call returns.
    pub(in crate::wasm) fn emit_tail_call_dispatch(&self, f: &mut Function) {
        let tc_signal = self.signal_local;
        let tc_payload = self.pay_local(Reg(0));
        let tc_tag = self.tag_local(Reg(0));
        // The I32 scratch trio sits after signal_local and suspended_local.
        let tc_is_wasm = self.signal_local + 2;
        let tc_table_idx = self.signal_local + 3;
        let tc_env_ptr = self.signal_local + 4;

        f.instruction(&Instruction::LocalSet(tc_signal));
        f.instruction(&Instruction::LocalSet(tc_payload));
        f.instruction(&Instruction::LocalSet(tc_tag));
        f.instruction(&Instruction::LocalSet(tc_is_wasm));
        f.instruction(&Instruction::LocalSet(tc_table_idx));
        f.instruction(&Instruction::LocalSet(tc_env_ptr));

        f.instruction(&Instruction::LocalGet(tc_is_wasm));
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            f.instruction(&Instruction::LocalGet(tc_env_ptr));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::LocalGet(tc_table_idx));
            f.instruction(&Instruction::ReturnCallIndirect {
                type_index: 5,
                table_index: 0,
            });
        }
        f.instruction(&Instruction::Else);
        {
            f.instruction(&Instruction::LocalGet(tc_tag));
            f.instruction(&Instruction::LocalGet(tc_payload));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);
    }
}
