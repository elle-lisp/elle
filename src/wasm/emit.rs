//! LIR → WASM emission.
//!
//! Converts a `LirFunction` into WASM module bytes using `wasm-encoder`.
//! Each LIR register maps to two WASM locals (tag: i64, payload: i64).
//! Immediate values (int, float, nil, bool) are constructed in WASM.
//! Heap operations go through host function calls.
//!
//! Split across files:
//! - `emit.rs` — module structure, WasmEmitter state, orchestration
//! - `instruction.rs` — LIR instruction → WASM instruction translation
//! - `controlflow.rs` — CFG emission, block dispatch, terminators
//! - `suspend.rs` — CPS suspension/resume, spill/restore, block splitting

use crate::lir::{Label, LirFunction, LirInstr, Reg, Terminator};
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
    /// Bytecode for each closure, indexed by table index.
    /// Used by spawn to execute WASM closures in new threads.
    pub closure_bytecodes: Vec<super::host::ClosureBytecode>,
}

/// Emit a WASM module from a top-level LirFunction.
pub fn emit_module(func: &LirFunction) -> EmitResult {
    let mut emitter = WasmEmitter::new();
    emitter.emit_module(func)
}

/// Emit a WASM module containing a single closure function.
///
/// Used by tiered compilation: the bytecode VM compiles individual hot
/// closures to WASM on demand. The module has the same host imports as
/// the full module but contains only one function (at table index 0).
///
/// Returns `None` if the function can't be compiled standalone (contains
/// MakeClosure, TailCall, or yield points).
pub fn emit_single_closure(func: &LirFunction) -> Option<EmitResult> {
    for block in &func.blocks {
        for si in &block.instructions {
            match &si.instr {
                LirInstr::MakeClosure { .. }
                | LirInstr::TailCall { .. }
                | LirInstr::TailCallArrayMut { .. } => return None,
                _ => {}
            }
        }
        if matches!(block.terminator.terminator, Terminator::Yield { .. }) {
            return None;
        }
    }

    let mut emitter = WasmEmitter::new();
    Some(emitter.emit_single_closure_module(func))
}

// Host function import indices (in order of declaration)
pub(super) const FN_RT_CALL: u32 = 1;
pub(super) const FN_RT_LOAD_CONST: u32 = 2;
pub(super) const FN_RT_DATA_OP: u32 = 3;
pub(super) const FN_RT_MAKE_CLOSURE: u32 = 4;
pub(super) const FN_RT_PUSH_PARAM: u32 = 5;
pub(super) const FN_RT_POP_PARAM: u32 = 6;
pub(super) const FN_RT_PREPARE_TAIL_CALL: u32 = 7;
pub(super) const FN_RT_YIELD: u32 = 8;
pub(super) const FN_RT_GET_RESUME_VALUE: u32 = 9;
pub(super) const FN_RT_LOAD_SAVED_REG: u32 = 10;

// First non-imported function index
pub(super) const FN_ENTRY: u32 = 11;

// Linear memory layout
pub(super) const ARGS_BASE: i32 = 256;

/// Data operation codes for rt_data_op.
///
/// These must stay in sync with `dispatch_data_op` in linker.rs.
#[repr(i32)]
#[derive(Clone, Copy)]
pub(super) enum DataOp {
    Cons = 0,
    Car = 1,
    Cdr = 2,
    CarDestructure = 3,
    CdrDestructure = 4,
    CarOrNil = 5,
    CdrOrNil = 6,
    MakeArray = 7,
    MakeLBox = 8,
    LoadLBox = 9,
    StoreLBox = 10,
    // 11 = MakeString (unused)
    ArrayRefDestructure = 12,
    ArraySliceFrom = 13,
    StructGetOrNil = 14,
    StructGetDestructure = 15,
    ArrayExtend = 16,
    ArrayPush = 17,
    ArrayLen = 18,
    ArrayRefOrNil = 19,
    StructRest = 20,
}

// Re-export as i32 constants for backward compat in instruction.rs
pub(super) const OP_CONS: i32 = DataOp::Cons as i32;
pub(super) const OP_CAR: i32 = DataOp::Car as i32;
pub(super) const OP_CDR: i32 = DataOp::Cdr as i32;
pub(super) const OP_CAR_DESTRUCTURE: i32 = DataOp::CarDestructure as i32;
pub(super) const OP_CDR_DESTRUCTURE: i32 = DataOp::CdrDestructure as i32;
pub(super) const OP_CAR_OR_NIL: i32 = DataOp::CarOrNil as i32;
pub(super) const OP_CDR_OR_NIL: i32 = DataOp::CdrOrNil as i32;
pub(super) const OP_MAKE_ARRAY: i32 = DataOp::MakeArray as i32;
pub(super) const OP_MAKE_LBOX: i32 = DataOp::MakeLBox as i32;
pub(super) const OP_LOAD_LBOX: i32 = DataOp::LoadLBox as i32;
pub(super) const OP_STORE_LBOX: i32 = DataOp::StoreLBox as i32;
pub(super) const OP_ARRAY_REF_DESTRUCTURE: i32 = DataOp::ArrayRefDestructure as i32;
pub(super) const OP_ARRAY_SLICE_FROM: i32 = DataOp::ArraySliceFrom as i32;
pub(super) const OP_STRUCT_GET_OR_NIL: i32 = DataOp::StructGetOrNil as i32;
pub(super) const OP_STRUCT_GET_DESTRUCTURE: i32 = DataOp::StructGetDestructure as i32;
pub(super) const OP_ARRAY_EXTEND: i32 = DataOp::ArrayExtend as i32;
pub(super) const OP_ARRAY_PUSH: i32 = DataOp::ArrayPush as i32;
pub(super) const OP_ARRAY_LEN: i32 = DataOp::ArrayLen as i32;
pub(super) const OP_ARRAY_REF_OR_NIL: i32 = DataOp::ArrayRefOrNil as i32;
pub(super) const OP_STRUCT_REST: i32 = DataOp::StructRest as i32;

/// Info about a resume state, used to generate the resume prologue.
pub(super) struct ResumeStateInfo {
    /// Resume state ID (1-based, passed as ctx).
    #[allow(dead_code)]
    pub state_id: u32,
    /// Block index to jump to after restoring registers.
    pub target_block_idx: i32,
    /// Number of saved register+local pairs.
    pub num_saved: u32,
}

/// Info about a call-site continuation (virtual resume block).
pub(super) struct CallSiteContinuation {
    /// The call's destination register.
    pub dst: Reg,
    /// Index of the original LIR block containing the call.
    pub source_block_idx: usize,
    /// Index of the first instruction AFTER the call in the source block.
    pub instr_offset: usize,
}

pub(super) struct WasmEmitter {
    pub label_to_idx: HashMap<Label, usize>,
    pub num_regs: u32,
    pub is_closure: bool,
    pub const_pool: Vec<Value>,
    pub closure_table_idx: HashMap<*const LirFunction, u32>,
    pub local_offset: u32,
    pub signal_local: u32,
    pub num_stack_locals: u32,
    pub may_suspend: bool,
    pub ctx_local: u32,
    pub next_resume_state: u32,
    pub resume_tag_local: u32,
    pub resume_pay_local: u32,
    pub resume_states: Vec<ResumeStateInfo>,
    pub call_continuations: Vec<CallSiteContinuation>,
    pub current_table_idx: u32,
    pub yield_state_map: HashMap<usize, u32>,
    pub call_state_map: HashMap<(usize, usize), u32>,
    pub reg_to_slot: HashMap<Reg, u32>,
    pub env_lbox_mask: u64,
    pub current_num_captures: u16,
    pub known_int: std::collections::HashSet<Reg>,
}

impl WasmEmitter {
    pub(super) fn new() -> Self {
        WasmEmitter {
            label_to_idx: HashMap::new(),
            num_regs: 0,
            is_closure: false,
            const_pool: Vec::new(),
            closure_table_idx: HashMap::new(),
            local_offset: 0,
            signal_local: 0,
            num_stack_locals: 0,
            may_suspend: false,
            ctx_local: 0,
            next_resume_state: 1,
            resume_tag_local: 0,
            resume_pay_local: 0,
            resume_states: Vec::new(),
            call_continuations: Vec::new(),
            yield_state_map: HashMap::new(),
            call_state_map: HashMap::new(),
            current_table_idx: 0,
            reg_to_slot: HashMap::new(),
            env_lbox_mask: 0,
            current_num_captures: 0,
            known_int: std::collections::HashSet::new(),
        }
    }

    /// Build the WASM type and import sections shared by all module variants.
    ///
    /// Type indices:
    ///   0: entry `(ctx: i32) -> (tag, payload, status)`
    ///   1: call_primitive `(prim_id, args_ptr, nargs, ctx) -> (tag, payload, signal)`
    ///   2: rt_call `(func_tag, func_payload, args_ptr, nargs, ctx) -> (tag, payload, signal)`
    ///   3: rt_load_const `(index) -> (tag, payload)`
    ///   4: rt_data_op `(op, args_ptr, nargs) -> (tag, payload, signal)`
    ///   5: closure `(env_ptr, args_ptr, nargs, ctx) -> (tag, payload, status)`
    ///   6: rt_make_closure `(table_idx, captures_ptr, metadata_ptr) -> (tag, payload)`
    ///   7: rt_push_param `(args_ptr, npairs) -> ()`
    ///   8: rt_pop_param `() -> ()`
    ///   9: rt_prepare_tail_call `(func_tag, func_payload, args_ptr, nargs, env_ptr) -> (env_ptr, table_idx, is_wasm, tag, payload, signal)`
    ///  10: rt_yield `(tag, payload, resume_state, regs_ptr, num_regs, func_idx, signal_bits) -> ()`
    ///  11: rt_get_resume_value `() -> (tag, payload)`
    ///  12: rt_load_saved_reg `(index) -> (tag, payload)`
    fn emit_types_and_imports(&self, module: &mut Module) {
        let mut types = TypeSection::new();
        // 0: entry function
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64, ValType::I32]);
        // 1: call_primitive
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // 2: rt_call
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
        // 3: rt_load_const
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        // 4: rt_data_op
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // 5: closure function
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // 6: rt_make_closure
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64],
        );
        // 7: rt_push_param
        types.ty().function([ValType::I32, ValType::I32], []);
        // 8: rt_pop_param
        types.ty().function([], []);
        // 9: rt_prepare_tail_call
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
        // 10: rt_yield
        types.ty().function(
            [
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [],
        );
        // 11: rt_get_resume_value
        types.ty().function([], [ValType::I64, ValType::I64]);
        // 12: rt_load_saved_reg
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        module.section(&types);

        let mut imports = ImportSection::new();
        imports.import("elle", "call_primitive", EntityType::Function(1));
        imports.import("elle", "rt_call", EntityType::Function(2));
        imports.import("elle", "rt_load_const", EntityType::Function(3));
        imports.import("elle", "rt_data_op", EntityType::Function(4));
        imports.import("elle", "rt_make_closure", EntityType::Function(6));
        imports.import("elle", "rt_push_param", EntityType::Function(7));
        imports.import("elle", "rt_pop_param", EntityType::Function(8));
        imports.import("elle", "rt_prepare_tail_call", EntityType::Function(9));
        imports.import("elle", "rt_yield", EntityType::Function(10));
        imports.import("elle", "rt_get_resume_value", EntityType::Function(11));
        imports.import("elle", "rt_load_saved_reg", EntityType::Function(12));
        module.section(&imports);
    }

    fn emit_module(&mut self, func: &LirFunction) -> EmitResult {
        let mut nested_funcs: Vec<&LirFunction> = Vec::new();
        collect_nested_functions(func, &mut nested_funcs);
        let num_closures = nested_funcs.len() as u32;

        self.closure_table_idx.clear();
        for (i, nf) in nested_funcs.iter().enumerate() {
            self.closure_table_idx
                .insert(*nf as *const LirFunction, i as u32);
        }

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
        let mut closure_bodies = Vec::with_capacity(nested_funcs.len());
        for (i, nested) in nested_funcs.iter().enumerate() {
            self.current_table_idx = i as u32;
            let closure_body = self.emit_closure_function(nested);
            closure_bodies.push(closure_body);
        }
        let entry_body = self.emit_function(func);
        let mut code = CodeSection::new();
        code.function(&entry_body);
        for closure_body in &closure_bodies {
            code.function(closure_body);
        }
        module.section(&code);

        // Dual-compile bytecode for spawn
        let mut closure_bytecodes = Vec::with_capacity(nested_funcs.len());
        let mut bc_emitter = crate::lir::Emitter::new();
        for nf in &nested_funcs {
            let (bytecode, _, _) = bc_emitter.emit(nf);
            closure_bytecodes.push((
                std::rc::Rc::new(bytecode.instructions),
                std::rc::Rc::new(bytecode.constants),
            ));
        }

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
            closure_bytecodes,
        }
    }

    fn emit_single_closure_module(&mut self, func: &LirFunction) -> EmitResult {
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

        let alloc = super::regalloc::allocate(func, func.num_locals as u32);
        let n = alloc.max_slots;
        self.reg_to_slot = alloc.reg_to_slot;
        self.num_regs = n;
        self.local_offset = 1;
        self.is_closure = false;
        self.may_suspend = false;
        self.ctx_local = 0;
        self.num_stack_locals = 0;
        self.signal_local = 1 + n * 2 + 1;

        let mut f = Function::new([
            (n, ValType::I64),
            (n, ValType::I64),
            (1, ValType::I32),
            (1, ValType::I32),
        ]);

        self.emit_cfg(&mut f, func);
        f.instruction(&Instruction::End);
        f
    }

    /// Emit a closure function body.
    pub(super) fn emit_closure_function(&mut self, func: &LirFunction) -> Function {
        let split_func;
        let func = if func.signal.may_suspend() {
            let mut orig_closures: Vec<*const LirFunction> = Vec::new();
            for block in &func.blocks {
                for si in &block.instructions {
                    if let LirInstr::MakeClosure { func: nf, .. } = &si.instr {
                        orig_closures.push(&**nf as *const LirFunction);
                    }
                }
            }

            let split_blocks = Self::split_blocks_at_suspending_calls(&func.blocks);

            let mut clone_idx = 0;
            for block in &split_blocks {
                for si in &block.instructions {
                    if let LirInstr::MakeClosure { func: nf, .. } = &si.instr {
                        let cloned_ptr = &**nf as *const LirFunction;
                        if clone_idx < orig_closures.len() {
                            let orig_ptr = orig_closures[clone_idx];
                            if let Some(&table_idx) = self.closure_table_idx.get(&orig_ptr) {
                                self.closure_table_idx.insert(cloned_ptr, table_idx);
                            }
                        }
                        clone_idx += 1;
                    }
                }
            }

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

        let alloc = super::regalloc::allocate(func, 0);
        let n = alloc.max_slots;
        if crate::config::get().debug_wasm {
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
            func.lbox_params_mask.wrapping_shl(nc as u32)
        };
        let np = nc + func.num_params as u64;
        let local_bits = if np >= 64 {
            u64::MAX
        } else {
            func.lbox_locals_mask.wrapping_shl(np as u32)
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

            if crate::config::get().debug_wasm {
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
                (4, ValType::I32),
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
                (4, ValType::I32),
            ]);
            self.emit_cfg(&mut f, func);
            f.instruction(&Instruction::End);
            f
        }
    }

    // --- Local accessors ---

    pub(super) fn env_local(&self) -> u32 {
        if self.local_offset == 0 {
            self.num_regs * 2
        } else {
            0
        }
    }

    pub(super) fn tag_local(&self, reg: Reg) -> u32 {
        let slot = self.reg_to_slot.get(&reg).copied().unwrap_or(reg.0);
        self.local_offset + slot
    }

    pub(super) fn pay_local(&self, reg: Reg) -> u32 {
        let slot = self.reg_to_slot.get(&reg).copied().unwrap_or(reg.0);
        self.local_offset + slot + self.num_regs
    }

    pub(super) fn tag_phys(&self, slot: u32) -> u32 {
        self.local_offset + slot
    }

    pub(super) fn pay_phys(&self, slot: u32) -> u32 {
        self.local_offset + slot + self.num_regs
    }

    pub(super) fn local_slot_tag(&self, slot: u16) -> u32 {
        self.local_offset + 2 * self.num_regs + slot as u32
    }

    pub(super) fn local_slot_pay(&self, slot: u16) -> u32 {
        self.local_offset + 2 * self.num_regs + self.num_stack_locals + slot as u32
    }
}

/// Recursively collect all nested LirFunctions from MakeClosure instructions.
fn collect_nested_functions<'a>(func: &'a LirFunction, out: &mut Vec<&'a LirFunction>) {
    for block in &func.blocks {
        for spanned in &block.instructions {
            if let LirInstr::MakeClosure { func: nested, .. } = &spanned.instr {
                out.push(nested);
                collect_nested_functions(nested, out);
            }
        }
    }
}
