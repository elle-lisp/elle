// audited: 2026-09-06
// docs/impl/wasm.md
//! LIR → WASM emission: the module's shape, the emitter's state, and what
//! drives one function body after another.
//!
//! Converts a `LirFunction` into WASM module bytes using `wasm-encoder`.
//! Each LIR register maps to two WASM locals (tag: i64, payload: i64).
//! Immediate values (int, float, nil, bool) are constructed in WASM.
//! Heap operations go through host function calls.
//!
//! Two files sit under this one: `abi` — the numbers the module and the host
//! agree on, and `functions` — the module sections and the two function-body
//! entry points.

use crate::lir::{ClosureId, Label, LirFunction, LirInstr, Reg, Terminator};
use crate::value::Value;
use std::collections::HashMap;
use wasm_encoder::*;

mod abi;
pub(in crate::wasm) use abi::*;

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
    /// Byte offset in linear memory where this module's env stack must begin.
    /// Sized above the module's widest args region so no call's args clobber a
    /// live closure env (see `env_stack_base`). The host initializes
    /// `ElleHost::env_stack_ptr` to this value.
    pub env_stack_base: usize,
}

/// Upper bound (in 16-byte slots) on the args region a single instruction
/// marshals at `ARGS_BASE` before a host call. The args region and the env stack
/// share linear memory — args grow up from `ARGS_BASE`, envs up from
/// `env_stack_base` — so the env stack must clear the widest such region or a
/// wide call's args overwrite a live closure env (a >128-field struct literal is
/// a >256-arg call to the `struct` primitive; `call-u16.lisp` and
/// `wasm::tests::frame::wasm_full_wide_call_from_closure_preserves_env` pin it).
///
/// A conservative over-estimate: it need not track each emitter's exact byte
/// layout (over-reserving costs only a little linear memory), only bound every
/// variable-length marshaling site. The fixed-arity ops (`emit_data_op1`/`2`,
/// `emit_call_array`, …) marshal at most a handful of slots, far under the
/// default 240-slot window, so they contribute 0 here.
fn instr_args_slots(instr: &LirInstr) -> usize {
    match instr {
        // Args written one 16-byte slot each (`emit_call` / the closure-tail path).
        LirInstr::Call { args, .. }
        | LirInstr::SuspendingCall { args, .. }
        | LirInstr::TailCall { args, .. } => args.len(),
        // Elements written one slot each (`emit_data_op_n`, OP_MAKE_ARRAY).
        LirInstr::MakeArrayMut { elements, .. } => elements.len(),
        // Captures, then 8 fixed meta slots plus the locals-mask words. The margin
        // covers the mask words (one per 64 captured locals) without a nested
        // closure lookup here.
        LirInstr::MakeClosure { captures, .. } => captures.len() + 72,
        // `src` slot plus one per excluded key (`OP_STRUCT_REST`).
        LirInstr::StructRest { exclude_keys, .. } => 1 + exclude_keys.len(),
        // Each pair is written with a 32-byte stride (two 16-byte slots).
        LirInstr::PushParamFrame { pairs } => pairs.len() * 2,
        _ => 0,
    }
}

/// Byte offset where `module`'s env stack must begin, given the widest args
/// region any of its functions marshals (see `instr_args_slots`). Floored at the
/// default `ENV_STACK_BASE` and page-aligned above the args region so a live
/// closure env never overlaps a call's args.
pub(super) fn env_stack_base(module: &crate::lir::LirModule) -> usize {
    let max_slots = std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .flat_map(|func| func.blocks.iter())
        .flat_map(|block| block.instructions.iter())
        .map(|si| instr_args_slots(&si.instr))
        .max()
        .unwrap_or(0);
    env_stack_base_for_slots(max_slots)
}

/// Env-stack base for a single function's widest args region — the tiered
/// per-closure path (`emit_single_closure_module`), which compiles one function
/// with no nested closures.
pub(super) fn env_stack_base_for_func(func: &LirFunction) -> usize {
    let max_slots = func
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .map(|si| instr_args_slots(&si.instr))
        .max()
        .unwrap_or(0);
    env_stack_base_for_slots(max_slots)
}

fn env_stack_base_for_slots(max_slots: usize) -> usize {
    // 4096-byte aligned above the args region — tidy page-ish base, and the
    // default `ENV_STACK_BASE` (4096) already carries a 240-slot window, so a
    // module with only ordinary-arity calls keeps the default.
    let needed = ARGS_BASE as usize + max_slots * 16;
    needed
        .next_multiple_of(4096)
        .max(super::host::ENV_STACK_BASE)
}

/// Emit a WASM module from an LirModule.
///
/// Closures in `stubbed` are emitted as minimal stubs (they have
/// pre-compiled standalone Modules and are dispatched via rt_call).
pub fn emit_module(
    module: &crate::lir::LirModule,
    stubbed: std::collections::HashSet<ClosureId>,
    heap_ptr: *mut crate::value::fiberheap::FiberHeap,
    symbols: *mut crate::symbol::SymbolTable,
) -> EmitResult {
    let mut emitter = WasmEmitter::new(heap_ptr);
    emitter.stubbed_closures = stubbed;
    emitter.symbols = symbols;
    emitter.emit_module_from_lir(module)
}

/// Whether `func` can be served as a standalone single-closure module.
///
/// A standalone module runs through hosts whose suspension and tail-call
/// imports are panic stubs (src/wasm/lazy/env.rs) and whose funcref table has a
/// single entry, so every shape whose execution would reach one of them is
/// refused here rather than detonated at runtime
/// (src/wasm/AGENTS.md § "Constraints on per-closure compilation"):
///
/// - `TailCall`/`TailCallArrayMut` — `return_call_indirect` needs callee
///   funcref-table indices and `rt_prepare_tail_call`;
/// - `SuspendingCall` and `Emit` terminators — every signal emission, yield
///   and `(error …)` alike, routes through `rt_yield`'s suspension-frame
///   machinery (a host-call error is unaffected: it returns via the status
///   word);
/// - `MakeClosure` without module context — no `ClosureId` resolution.
///
/// Pinned by `wasm::tests::gate::standalone_emission_refuses_*`.
fn standalone_emittable(func: &LirFunction, has_module_context: bool) -> bool {
    func.blocks.iter().all(|b| {
        b.instructions.iter().all(|si| match &si.instr {
            LirInstr::TailCall { .. }
            | LirInstr::TailCallArrayMut { .. }
            | LirInstr::SuspendingCall { .. } => false,
            LirInstr::MakeClosure { .. } => has_module_context,
            _ => true,
        }) && !matches!(b.terminator.terminator, Terminator::Emit { .. })
    })
}

/// Emit a standalone WASM module for a single closure (at table index 0).
///
/// Used by tiered compilation (the bytecode VM compiles individual hot
/// closures on demand) and by per-closure precaching. The module has the same
/// host imports as the full module but contains only one function. Returns
/// `None` for a closure the standalone hosts cannot serve
/// (`standalone_emittable` above); the tiered caller falls back to the
/// bytecode VM, the precache caller to full-module dispatch.
pub fn emit_single_closure(
    func: &LirFunction,
    module: Option<&crate::lir::LirModule>,
    heap_ptr: *mut crate::value::fiberheap::FiberHeap,
    symbols: *mut crate::symbol::SymbolTable,
) -> Option<EmitResult> {
    if !standalone_emittable(func, module.is_some()) {
        return None;
    }
    let mut emitter = WasmEmitter::new(heap_ptr);
    emitter.symbols = symbols;
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

/// Info about a resume state, used to generate the resume prologue.
pub(super) struct ResumeStateInfo {
    /// Resume state ID (1-based, passed as ctx).
    #[allow(dead_code)]
    pub state_id: u32,
    /// Block index to jump to after restoring registers.
    pub target_block_idx: i32,
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
    /// The driving instance's heap, on which compile-time const-pool Values
    /// (string / compound literals baked into the module) are built. The module
    /// holds these for its lifetime, so they live on the instance heap that
    /// outlives it.
    pub heap_ptr: *mut crate::value::fiberheap::FiberHeap,
    /// The driving instance's symbol table, needed to intern a quoted *symbol*
    /// leaf when baking a compound literal into the const pool
    /// (`ConstTemplate::materialize`). Null on the standalone/tiered path
    /// (`lazy.rs`), which has no instance table in scope; a compound-symbol
    /// literal there would panic in `materialize`, so `standalone_emittable`
    /// keeps such closures on the bytecode VM. The full-module path always sets
    /// it (src/wasm/tests/stdlib.rs).
    pub symbols: *mut crate::symbol::SymbolTable,
}

mod functions;

impl WasmEmitter {
    pub(super) fn new(heap_ptr: *mut crate::value::fiberheap::FiberHeap) -> Self {
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
            heap_ptr,
            symbols: std::ptr::null_mut(),
        }
    }

    /// Build the WASM type and import sections shared by all module variants.
    ///
    /// Type indices:
    ///   0: entry `(ctx: i32) -> (tag, payload, status)`
    ///   1: call_primitive `(prim_id, args_ptr, nargs, ctx) -> (tag, payload, signal)`
    ///   2: rt_call `(func_tag, func_payload, args_ptr, nargs, ctx) -> (tag, payload, signal, suspended)`
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
        // 2: rt_call. The fourth result is `suspended`: whether the caller must
        // park. It is a separate word from `signal` because which signals park
        // is the interpreter's rule (`signals::dispatch::is_suspending`), not a
        // bit the emitter can test — see docs/impl/wasm.md § rt_call.
        types.ty().function(
            [
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [ValType::I64, ValType::I64, ValType::I64, ValType::I64],
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

    // --- Local accessors ---

    pub(super) fn env_local(&self) -> u32 {
        if self.local_offset == 0 {
            self.num_regs * 2
        } else {
            0
        }
    }

    /// The local holding `rt_call`'s fourth result: whether the callee parked.
    ///
    /// Declared immediately after `signal_local` in every function variant
    /// (`emit_function` / `emit_closure_function`), so the two words that
    /// describe one call sit together. The tail-call scratch follows it.
    pub(in crate::wasm) fn suspended_local(&self) -> u32 {
        self.signal_local + 1
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
