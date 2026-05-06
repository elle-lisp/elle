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

use crate::lir::{ClosureId, Label, LirFunction, Reg};
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

/// Emit a WASM module from an LirModule.
///
/// Closures in `stubbed` are emitted as minimal stubs (they have
/// pre-compiled standalone Modules and are dispatched via rt_call).
pub fn emit_module(
    module: &crate::lir::LirModule,
    stubbed: std::collections::HashSet<ClosureId>,
) -> EmitResult {
    let mut emitter = WasmEmitter::new();
    emitter.stubbed_closures = stubbed;
    emitter.emit_module_from_lir(module)
}

/// Emit a WASM module containing a single closure function.
///
/// Used by tiered compilation: the bytecode VM compiles individual hot
/// closures to WASM on demand. The module has the same host imports as
/// the full module but contains only one function (at table index 0).
///
/// Emit a standalone WASM module for a single closure.
///
/// All instruction types are supported:
/// - MakeClosure/TailCall via host-mediated dispatch
/// - Yield via CPS transform (same as full module) + rt_yield on host
pub fn emit_single_closure(
    func: &LirFunction,
    module: Option<&crate::lir::LirModule>,
) -> Option<EmitResult> {
    let mut emitter = WasmEmitter::new();
    // Provide module context for MakeClosure → ClosureId resolution
    if let Some(m) = module {
        emitter.module_closures = Some(m.closures.clone());
        // In standalone mode, closure_id maps to table index 0
        // (the closure itself). Nested closures go through rt_make_closure
        // which creates host-side Closure values dispatched via rt_call.
        // The table_idx in rt_make_closure is used by the host, not the table.
        for i in 0..m.closures.len() {
            emitter
                .closure_id_to_table_idx
                .insert(ClosureId(i as u32), i as u32);
        }
    }
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
    Pair = 0,
    First = 1,
    Rest = 2,
    FirstDestructure = 3,
    RestDestructure = 4,
    FirstOrNil = 5,
    RestOrNil = 6,
    MakeArray = 7,
    MakeCapture = 8,
    LoadCapture = 9,
    StoreCapture = 10,
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
    IntToFloat = 21,
    FloatToInt = 22,
    IntrTypeOf = 23,
    IntrLength = 24,
    IntrGetOp = 25,
    IntrPutOp = 26,
    IntrDelOp = 27,
    IntrHasOp = 28,
    IntrPopOp = 29,
    IntrFreezeOp = 30,
    IntrThawOp = 31,
    IntrIdenticalOp = 32,
    IntrPushOp = 33,
}

// Re-export as i32 constants for backward compat in instruction.rs
pub(super) const OP_CONS: i32 = DataOp::Pair as i32;
pub(super) const OP_CAR: i32 = DataOp::First as i32;
pub(super) const OP_CDR: i32 = DataOp::Rest as i32;
pub(super) const OP_CAR_DESTRUCTURE: i32 = DataOp::FirstDestructure as i32;
pub(super) const OP_CDR_DESTRUCTURE: i32 = DataOp::RestDestructure as i32;
pub(super) const OP_CAR_OR_NIL: i32 = DataOp::FirstOrNil as i32;
pub(super) const OP_CDR_OR_NIL: i32 = DataOp::RestOrNil as i32;
pub(super) const OP_MAKE_ARRAY: i32 = DataOp::MakeArray as i32;
pub(super) const OP_MAKE_CAPTURE: i32 = DataOp::MakeCapture as i32;
pub(super) const OP_LOAD_CAPTURE: i32 = DataOp::LoadCapture as i32;
pub(super) const OP_STORE_CAPTURE: i32 = DataOp::StoreCapture as i32;
pub(super) const OP_ARRAY_REF_DESTRUCTURE: i32 = DataOp::ArrayRefDestructure as i32;
pub(super) const OP_ARRAY_SLICE_FROM: i32 = DataOp::ArraySliceFrom as i32;
pub(super) const OP_STRUCT_GET_OR_NIL: i32 = DataOp::StructGetOrNil as i32;
pub(super) const OP_STRUCT_GET_DESTRUCTURE: i32 = DataOp::StructGetDestructure as i32;
pub(super) const OP_ARRAY_EXTEND: i32 = DataOp::ArrayExtend as i32;
pub(super) const OP_ARRAY_PUSH: i32 = DataOp::ArrayPush as i32;
pub(super) const OP_ARRAY_LEN: i32 = DataOp::ArrayLen as i32;
pub(super) const OP_ARRAY_REF_OR_NIL: i32 = DataOp::ArrayRefOrNil as i32;
pub(super) const OP_STRUCT_REST: i32 = DataOp::StructRest as i32;
pub(super) const OP_INT_TO_FLOAT: i32 = DataOp::IntToFloat as i32;
pub(super) const OP_FLOAT_TO_INT: i32 = DataOp::FloatToInt as i32;
pub(super) const OP_TYPE_OF: i32 = DataOp::IntrTypeOf as i32;
pub(super) const OP_LENGTH: i32 = DataOp::IntrLength as i32;
pub(super) const OP_INTR_GET: i32 = DataOp::IntrGetOp as i32;
pub(super) const OP_INTR_PUT: i32 = DataOp::IntrPutOp as i32;
pub(super) const OP_INTR_DEL: i32 = DataOp::IntrDelOp as i32;
pub(super) const OP_INTR_HAS: i32 = DataOp::IntrHasOp as i32;
pub(super) const OP_INTR_POP: i32 = DataOp::IntrPopOp as i32;
pub(super) const OP_INTR_FREEZE: i32 = DataOp::IntrFreezeOp as i32;
pub(super) const OP_INTR_THAW: i32 = DataOp::IntrThawOp as i32;
pub(super) const OP_INTR_IDENTICAL: i32 = DataOp::IntrIdenticalOp as i32;
pub(super) const OP_INTR_PUSH: i32 = DataOp::IntrPushOp as i32;

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
    pub closure_id_to_table_idx: HashMap<ClosureId, u32>,
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
    /// Module's closure list for MakeClosure metadata lookup.
    pub module_closures: Option<Vec<LirFunction>>,
    /// Closures to emit as stubs (pre-compiled as standalone Modules).
    pub stubbed_closures: std::collections::HashSet<ClosureId>,
    /// Per-suspend-point live register sets for sparse spilling.
    pub spill_live_map: super::liveness::SpillLiveMap,
}

impl WasmEmitter {
    pub(super) fn new() -> Self {
        WasmEmitter {
            label_to_idx: HashMap::new(),
            num_regs: 0,
            is_closure: false,
            const_pool: Vec::new(),
            closure_id_to_table_idx: HashMap::new(),
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
            module_closures: None,
            stubbed_closures: std::collections::HashSet::new(),
            spill_live_map: HashMap::new(),
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
            .function([ValType::I32], [ValType::I64, ValType::I64, ValType::I64]);
        // 1: call_primitive
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I64],
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
            [ValType::I64, ValType::I64, ValType::I64],
        );
        // 3: rt_load_const
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        // 4: rt_data_op
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I64],
        );
        // 5: closure function
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I64],
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
                ValType::I64,
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
                ValType::I64,
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

        let alloc = super::regalloc::allocate(func, 0);
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
        let local_bits = if np >= 64 {
            u64::MAX
        } else {
            func.capture_locals_mask.wrapping_shl(np as u32)
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
                self.spill_live_map = super::liveness::compute_spill_liveness(
                    func,
                    &self.label_to_idx,
                    &self.reg_to_slot,
                    n,
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
