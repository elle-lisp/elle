// audited: 2026-10-06
//! The module sections, and the two function bodies an emitter produces: the entry and a closure.
//!
//! docs/impl/wasm.md

use super::*;

impl WasmEmitter {
    pub(super) fn emit_module_from_lir(&mut self, lir_module: &FrozenModule) -> EmitResult {
        let num_closures = lir_module.closures.len() as u32;

        self.closure_id_to_table_idx.clear();
        for i in 0..lir_module.closures.len() {
            self.closure_id_to_table_idx
                .insert(ClosureId(i as u32), i as u32);
        }
        self.module_closures = Some(lir_module.closures.clone());

        let mut module = Module::new();
        self.emit_types_and_imports(&mut module);

        // Function section
        let mut functions = FunctionSection::new();
        functions.function(0);
        for _ in 0..num_closures {
            functions.function(5);
        }
        module.section(&functions);

        // Table section
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

        // Element section
        if num_closures > 0 {
            let mut elements = ElementSection::new();
            let func_indices: Vec<u32> = (0..num_closures).map(|i| FN_ENTRY + 1 + i).collect();
            elements.active(
                Some(0),
                &ConstExpr::i32_const(0),
                Elements::Functions(func_indices.into()),
            );
            module.section(&elements);
        }

        // Code section
        //
        // Emit closures BEFORE the entry function so that stdlib closure
        // constants get stable pool indices regardless of user code.
        // Wasmtime's incremental compilation cache keys on per-function
        // WASM bytes, so stable indices → cache hits across programs.
        // The code section must list functions in declaration order
        // (entry first), so we buffer the closure bodies.
        let mut closure_bodies = Vec::with_capacity(lir_module.closures.len());
        for (i, closure_func) in lir_module.closures.iter().enumerate() {
            self.current_table_idx = i as u32;
            if self.stubbed_closures.contains(&ClosureId(i as u32)) {
                // Emit a minimal stub — this closure is pre-compiled
                // as a standalone Module and dispatched via rt_call.
                // The stub is never reached at runtime.
                let mut stub =
                    Function::new([(1, ValType::I64), (1, ValType::I64), (1, ValType::I64)]);
                stub.instruction(&Instruction::Unreachable);
                stub.instruction(&Instruction::End);
                closure_bodies.push(stub);
            } else {
                let closure_body = self.emit_closure_function(&closure_func.view());
                closure_bodies.push(closure_body);
            }
        }
        let entry_body = self.emit_function(&lir_module.entry.view());
        let mut code = CodeSection::new();
        code.function(&entry_body);
        for closure_body in &closure_bodies {
            code.function(closure_body);
        }
        module.section(&code);

        // Dual-compile bytecode for spawn, into a code unit of the module's own
        // on the driving instance's heap. Each closure's payload carries its
        // child table: the bytecode's MakeClosure instructions index it, so a
        // spawned worker building a code object from this needs it
        // (rt_make_closure, src/wasm/linker/create/closure.rs). Without it the
        // worker panics on its first MakeClosure (`wasm::tests::closure`).
        let code = crate::value::CodeArena::mint(unsafe { &mut *self.heap_ptr });
        let ((entry, _, _), lambdas) =
            crate::lir::Emitter::new(code).emit_module_with_lambdas(lir_module);
        let closure_bytecodes =
            super::super::host::ModuleCode::new(crate::value::CodeUnit::new(code, entry), lambdas);

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
            closure_bytecodes,
            env_stack_base: super::env_stack_base(lir_module),
        }
    }
    pub(super) fn emit_single_closure_module(&mut self, func: &LirView<'_>) -> EmitResult {
        let mut module = Module::new();
        self.emit_types_and_imports(&mut module);

        let mut functions = FunctionSection::new();
        functions.function(5);
        module.section(&functions);

        let mut tables = TableSection::new();
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            minimum: 1,
            maximum: Some(1),
            shared: false,
            table64: false,
        });
        module.section(&tables);

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
        exports.export("__elle_closure", ExportKind::Func, FN_ENTRY);
        exports.export("__elle_memory", ExportKind::Memory, 0);
        exports.export("__elle_table", ExportKind::Table, 0);
        module.section(&exports);

        let mut elements = ElementSection::new();
        elements.active(
            Some(0),
            &ConstExpr::i32_const(0),
            Elements::Functions(vec![FN_ENTRY].into()),
        );
        module.section(&elements);

        let mut code = CodeSection::new();
        self.current_table_idx = 0;
        let closure_body = self.emit_closure_function(func);
        code.function(&closure_body);
        module.section(&code);

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
            closure_bytecodes: super::super::host::ModuleCode::none(),
            env_stack_base: super::env_stack_base_for_func(func),
        }
    }
    /// Emit the entry function body.
    pub(super) fn emit_function(&mut self, func: &LirView<'_>) -> Function {
        self.label_to_idx.clear();
        for (idx, block) in func.blocks().enumerate() {
            self.label_to_idx.insert(block.label(), idx);
        }

        let alloc = super::super::regalloc::allocate(func, func.num_locals() as u32);
        let n = alloc.max_slots;
        self.reg_to_slot = alloc.reg_to_slot;
        self.num_regs = n;
        self.local_offset = 1;
        self.is_closure = false;
        self.may_suspend = false;
        self.ctx_local = 0;
        self.num_stack_locals = 0;
        self.signal_local = 1 + n * 2 + 1;
        // Reset the suspend/resume scratch that `emit_cfg` consumes. Closures are
        // emitted before the entry (emit_module_from_lir), and a suspending
        // closure leaves `call_continuations` populated with offsets into ITS
        // blocks. `emit_cfg` emits one virtual resume block per continuation and
        // slices `func.blocks[..][instr_offset..]`; against the entry's own
        // (unrelated, shorter) blocks a stale offset panics. The entry does not
        // suspend to its host caller (`may_suspend = false` above), so the
        // correct state is empty. Pinned by tests/lang/bug-propagate-free-at.lisp
        // under `--wasm=full` (which produces multiple suspending `ev/run` thunks
        // ahead of a short entry).
        self.next_resume_state = 1;
        self.resume_states.clear();
        self.call_continuations.clear();
        self.yield_state_map.clear();
        self.call_state_map.clear();

        // The trailing I64 pair is `signal_local` then `suspended_local`
        // (`WasmEmitter::suspended_local`); they must stay adjacent and last.
        let mut f = Function::new([
            (n, ValType::I64),
            (n, ValType::I64),
            (1, ValType::I32),
            (2, ValType::I64),
        ]);

        self.emit_cfg(&mut f, func);
        f.instruction(&Instruction::End);
        f
    }
    /// Emit a closure function body.
    pub(super) fn emit_closure_function(&mut self, func: &LirView<'_>) -> Function {
        // A suspending closure ends a block after each suspending call, so its
        // CPS resume blocks start at a block boundary rather than copying the
        // rest of the block once per call.
        let split;
        let split_view;
        let func = if func.signal().may_suspend() {
            split = func.split_after(|op| matches!(op, Op::SuspendingCall | Op::CallArrayMut));
            split_view = split.view();
            &split_view
        } else {
            func
        };

        self.label_to_idx.clear();
        for (idx, block) in func.blocks().enumerate() {
            self.label_to_idx.insert(block.label(), idx);
        }

        // Reset the suspend/resume scratch that `emit_cfg` consumes. Closures are
        // emitted in sequence, so a preceding SUSPENDING closure leaves
        // `call_continuations`/`resume_states` populated with offsets into ITS
        // blocks. A NON-suspending closure never repopulates them (only the
        // `may_suspend` path's `pre_scan_resume_states` does), so without this
        // reset `emit_cfg` reads the stale `call_continuations.len()` as this
        // function's virtual-block count — inflating `total_blocks` into a giant
        // br_table and slicing `func.blocks[stale_src][stale_offset..]` against
        // this function's unrelated (shorter) blocks. The entry function resets
        // the same scratch for the same reason. Pinned by
        // `tests/impl/region-capture-cell-loop-uaf.lisp` under `--wasm=full`.
        self.next_resume_state = 1;
        self.resume_states.clear();
        self.call_continuations.clear();
        self.yield_state_map.clear();
        self.call_state_map.clear();

        let alloc = super::super::regalloc::allocate(func, 0);
        let n = alloc.max_slots;
        if crate::config::get().has_trace("wasm") {
            eprintln!(
                "[emit] closure {:?}: {} virtual regs → {} slots",
                func.name(),
                func.num_regs(),
                n
            );
        }
        self.reg_to_slot = alloc.reg_to_slot;
        self.num_regs = n;
        self.local_offset = 4;
        self.is_closure = true;
        self.ctx_local = 3;
        self.num_stack_locals = func.num_locals() as u32;
        self.may_suspend = func.signal().may_suspend();
        self.current_num_captures = func.num_captures();

        let m = self.num_stack_locals;
        self.signal_local = 4 + 2 * n + 2 * m;

        if self.may_suspend {
            // Past signal_local, suspended_local, and the three I32 tail-call
            // scratch locals (see `emit_tail_call_dispatch`).
            self.resume_tag_local = 4 + 2 * n + 2 * m + 5;
            self.resume_pay_local = 4 + 2 * n + 2 * m + 6;

            self.pre_scan_resume_states(func);
            self.next_resume_state = 1;

            // Compute per-suspend-point liveness for sparse spilling.
            if crate::config::get().wasm_sparse_spill {
                self.spill_live_map = super::super::liveness::compute_spill_liveness(
                    func,
                    &self.label_to_idx,
                    &self.reg_to_slot,
                    n,
                    self.num_stack_locals,
                );
            } else {
                self.spill_live_map.clear();
            }

            if crate::config::get().has_trace("wasm") {
                eprintln!(
                    "[emit] suspending closure: name={:?} regs={} locals={} captures={} params={}",
                    func.name(),
                    func.num_regs(),
                    func.num_locals(),
                    func.num_captures(),
                    func.num_params()
                );
                for block in func.blocks() {
                    eprintln!("[emit]   Block {:?}:", block.label());
                    for instr in block.instrs() {
                        eprintln!("[emit]     {:?}", instr);
                    }
                    eprintln!("[emit]     term: {:?}", block.terminator());
                }
            }

            let mut f = Function::new([
                (n, ValType::I64),
                (n, ValType::I64),
                (m, ValType::I64),
                (m, ValType::I64),
                (2, ValType::I64),
                (3, ValType::I32),
                (2, ValType::I64),
            ]);
            self.emit_cfg(&mut f, func);
            f.instruction(&Instruction::End);
            f
        } else {
            let mut f = Function::new([
                (n, ValType::I64),
                (n, ValType::I64),
                (m, ValType::I64),
                (m, ValType::I64),
                (2, ValType::I64),
                (3, ValType::I32),
            ]);
            self.emit_cfg(&mut f, func);
            f.instruction(&Instruction::End);
            f
        }
    }
}
