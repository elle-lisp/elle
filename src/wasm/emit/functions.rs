use super::*;

impl WasmEmitter {
    pub(super) fn emit_module_from_lir(
        &mut self,
        lir_module: &crate::lir::LirModule,
    ) -> EmitResult {
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
                let closure_body = self.emit_closure_function(closure_func);
                closure_bodies.push(closure_body);
            }
        }
        let entry_body = self.emit_function(&lir_module.entry);
        let mut code = CodeSection::new();
        code.function(&entry_body);
        for closure_body in &closure_bodies {
            code.function(closure_body);
        }
        module.section(&code);

        // Dual-compile bytecode for spawn.
        // Use emit_module which handles MakeClosure → ClosureId resolution.
        let mut bc_emitter = crate::lir::Emitter::new();
        let bc_compiled = bc_emitter.emit_module_closures(lir_module);
        let mut closure_bytecodes = Vec::with_capacity(bc_compiled.len());
        for (bytecode, _, _) in bc_compiled {
            // Carry child_protos: the bytecode's MakeClosure instructions index
            // this list, so a spawned worker reconstructing the template needs it
            // (rt_make_closure, src/wasm/linker/create/closure.rs). Dropping it
            // left the template's child list empty and the worker panicked.
            closure_bytecodes.push((
                std::rc::Rc::new(bytecode.instructions),
                std::rc::Rc::new(bytecode.constants),
                std::rc::Rc::new(bytecode.child_protos),
            ));
        }

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
            closure_bytecodes,
        }
    }
    pub(super) fn emit_single_closure_module(&mut self, func: &LirFunction) -> EmitResult {
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
            closure_bytecodes: Vec::new(),
        }
    }
    /// Emit the entry function body.
    pub(super) fn emit_function(&mut self, func: &LirFunction) -> Function {
        self.label_to_idx.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        let alloc = super::super::regalloc::allocate(func, func.num_locals as u32);
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
        // correct state is empty. Pinned by tests/elle/bug-propagate-free-at.lisp
        // under `--wasm=full` (which produces multiple suspending `ev/run` thunks
        // ahead of a short entry).
        self.next_resume_state = 1;
        self.resume_states.clear();
        self.call_continuations.clear();
        self.yield_state_map.clear();
        self.call_state_map.clear();

        let mut f = Function::new([
            (n, ValType::I64),
            (n, ValType::I64),
            (1, ValType::I32),
            (1, ValType::I64),
        ]);

        self.emit_cfg(&mut f, func);
        f.instruction(&Instruction::End);
        f
    }
    /// Emit a closure function body.
    pub(super) fn emit_closure_function(&mut self, func: &LirFunction) -> Function {
        let split_func;
        let func = if func.signal.may_suspend() {
            // ClosureId is Copy and survives block splitting/cloning
            // — no pointer remapping needed.
            let split_blocks = Self::split_blocks_at_suspending_calls(&func.blocks);
            split_func = LirFunction {
                blocks: split_blocks,
                ..func.clone()
            };
            &split_func
        } else {
            func
        };

        self.label_to_idx.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        let alloc = super::super::regalloc::allocate(func, 0);
        let n = alloc.max_slots;
        if crate::config::get().has_trace("wasm") {
            eprintln!(
                "[emit] closure {:?}: {} virtual regs → {} slots",
                func.name, func.num_regs, n
            );
        }
        self.reg_to_slot = alloc.reg_to_slot;
        self.num_regs = n;
        self.local_offset = 4;
        self.is_closure = true;
        self.ctx_local = 3;
        self.num_stack_locals = func.num_locals as u32;
        self.may_suspend = func.signal.may_suspend();
        self.current_num_captures = func.num_captures;
        // Build LBox mask
        let nc = func.num_captures as u64;
        let capture_bits = if nc >= 64 { u64::MAX } else { (1u64 << nc) - 1 };
        let param_bits = if nc >= 64 {
            u64::MAX
        } else {
            func.capture_params_mask.wrapping_shl(nc as u32)
        };
        let np = nc + func.num_params as u64;
        // `env_lbox_mask` is a u64 view kept for compatibility; its low-64 width
        // is unchanged by the `CaptureMask` widening (the field is currently
        // unread). Take the low-64 bits of the locals mask before shifting.
        let local_bits = if np >= 64 {
            u64::MAX
        } else {
            func.capture_locals_mask.low_u64().wrapping_shl(np as u32)
        };
        self.env_lbox_mask = capture_bits | param_bits | local_bits;
        self.next_resume_state = 1;
        self.resume_states.clear();
        self.call_continuations.clear();

        let m = self.num_stack_locals;
        self.signal_local = 4 + 2 * n + 2 * m;

        if self.may_suspend {
            self.resume_tag_local = 4 + 2 * n + 2 * m + 4;
            self.resume_pay_local = 4 + 2 * n + 2 * m + 5;

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
                    func.name, func.num_regs, func.num_locals, func.num_captures, func.num_params
                );
                for block in &func.blocks {
                    eprintln!("[emit]   Block {:?}:", block.label);
                    for si in &block.instructions {
                        eprintln!("[emit]     {:?}", si.instr);
                    }
                    eprintln!("[emit]     term: {:?}", block.terminator.terminator);
                }
            }

            let mut f = Function::new([
                (n, ValType::I64),
                (n, ValType::I64),
                (m, ValType::I64),
                (m, ValType::I64),
                (1, ValType::I64),
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
                (1, ValType::I64),
                (3, ValType::I32),
            ]);
            self.emit_cfg(&mut f, func);
            f.instruction(&Instruction::End);
            f
        }
    }
}
