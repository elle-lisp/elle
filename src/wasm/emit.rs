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
use crate::value::repr::*;
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
    // Reject functions that can't be self-contained in a single-function module:
    // - MakeClosure: would need nested functions in the table
    // - TailCall/TailCallArrayMut: uses return_call_indirect with callee table indices
    // - Yield: needs suspension frame management tied to the full module
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
const _FN_CALL_PRIMITIVE: u32 = 0;
const FN_RT_CALL: u32 = 1;
const FN_RT_LOAD_CONST: u32 = 2;
const FN_RT_DATA_OP: u32 = 3;
const FN_RT_MAKE_CLOSURE: u32 = 4;
const FN_RT_PUSH_PARAM: u32 = 5;
const FN_RT_POP_PARAM: u32 = 6;
const FN_RT_PREPARE_TAIL_CALL: u32 = 7;
const FN_RT_YIELD: u32 = 8;
const FN_RT_GET_RESUME_VALUE: u32 = 9;
const FN_RT_LOAD_SAVED_REG: u32 = 10;

// First non-imported function index
const FN_ENTRY: u32 = 11;

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
const OP_STRUCT_REST: i32 = 20;

/// Info about a resume state, used to generate the resume prologue.
struct ResumeStateInfo {
    /// Resume state ID (1-based, passed as ctx).
    #[allow(dead_code)]
    state_id: u32,
    /// Block index to jump to after restoring registers.
    /// For yield terminators: the resume_label's block index.
    /// For call sites: the virtual block index (num_real_blocks + continuation_idx).
    target_block_idx: i32,
    /// Number of saved register+local pairs.
    num_saved: u32,
}

/// Info about a call-site continuation (virtual resume block).
/// When a callee yields through a call, the caller resumes in a
/// virtual block that loads the resume value into the call dst
/// and emits the remaining instructions from the original block.
struct CallSiteContinuation {
    /// The call's destination register.
    dst: Reg,
    /// Index of the original LIR block containing the call.
    source_block_idx: usize,
    /// Index of the first instruction AFTER the call in the source block.
    instr_offset: usize,
}

struct WasmEmitter {
    label_to_idx: HashMap<Label, usize>,
    num_regs: u32,
    /// Whether we're currently emitting a closure function (vs entry).
    is_closure: bool,
    /// Heap constants collected during emission.
    const_pool: Vec<Value>,
    /// Maps nested LirFunction pointer → table index.
    /// Populated during collect_nested_functions, used by MakeClosure emission.
    closure_table_idx: HashMap<*const LirFunction, u32>,
    /// Offset for register locals (0 for entry, 4 for closures with params).
    local_offset: u32,
    /// WASM local index for signal scratch (i32).
    signal_local: u32,
    /// Number of stack-local slots (non-LBox let-bound variables in closures).
    num_stack_locals: u32,
    /// Whether the current function may suspend (yield or yield-through-call).
    may_suspend: bool,
    /// Next resume state ID for yield points and call sites.
    /// State 0 = initial entry; states 1+ are assigned to yield/call points.
    next_resume_state: u32,
    /// WASM local index for resume value tag (i64). Set during resume prologue.
    resume_tag_local: u32,
    /// WASM local index for resume value payload (i64).
    resume_pay_local: u32,
    /// Resume state table: maps state IDs to target blocks and saved counts.
    /// Built during yield/call-site emission, consumed by resume prologue.
    resume_states: Vec<ResumeStateInfo>,
    /// Call-site continuations for yield-through-call virtual blocks.
    call_continuations: Vec<CallSiteContinuation>,
    /// Table index of the current closure being emitted.
    /// Used by rt_yield to record which function actually yielded (important
    /// when tail calls replace the caller's frame).
    current_table_idx: u32,
    /// Maps (block_idx) → resume_state for yield terminators.
    yield_state_map: HashMap<usize, u32>,
    /// Maps (block_idx, instr_idx) → resume_state for call sites.
    call_state_map: HashMap<(usize, usize), u32>,
    /// Register allocation map: LIR Reg → compacted WASM local slot.
    reg_to_slot: HashMap<Reg, u32>,
    /// Bitmask of env slots that are LBox cells.
    /// Derived from the current closure's lbox_params_mask and lbox_locals_mask.
    /// Used to skip dead LBox unwrap checks in LoadCapture.
    env_lbox_mask: u64,
    /// Number of captures in the current closure (env layout offset for params).
    current_num_captures: u16,
    /// Registers known to hold integer values (TAG_INT) at the current point.
    /// Used to skip float dispatch in BinOp/Compare.
    /// Cleared at block boundaries (conservative).
    known_int: std::collections::HashSet<Reg>,
}

impl WasmEmitter {
    fn new() -> Self {
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

    fn emit_module(&mut self, func: &LirFunction) -> EmitResult {
        // Phase 1: collect all nested LirFunctions by walking MakeClosure instructions.
        // Each gets a table index. The entry function is NOT in the table.
        let mut nested_funcs: Vec<&LirFunction> = Vec::new();
        collect_nested_functions(func, &mut nested_funcs);
        let num_closures = nested_funcs.len() as u32;

        // Build pointer → table index map for MakeClosure emission
        self.closure_table_idx.clear();
        for (i, nf) in nested_funcs.iter().enumerate() {
            self.closure_table_idx
                .insert(*nf as *const LirFunction, i as u32);
        }

        let mut module = Module::new();

        // Type section
        let mut types = TypeSection::new();
        // Type 0: entry function () -> (i64, i64, i32)
        // status: 0 = normal return, >0 = suspended (resume state ID)
        types
            .ty()
            .function([], [ValType::I64, ValType::I64, ValType::I32]);
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
        // Type 5: closure function (env_ptr, args_ptr, nargs, ctx) -> (tag, payload, status)
        // status: 0 = normal return, >0 = suspended (resume state ID)
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // Type 6: rt_make_closure(table_idx, captures_ptr, metadata_ptr) -> (tag, payload)
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64],
        );
        // Type 7: rt_push_param(args_ptr, npairs) -> ()
        types.ty().function([ValType::I32, ValType::I32], []);
        // Type 8: rt_pop_param() -> ()
        types.ty().function([], []);
        // Type 9: rt_prepare_tail_call(func_tag, func_payload, args_ptr, nargs, env_ptr)
        //   -> (env_ptr, table_idx, is_wasm, tag, payload, signal)
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
        // Type 10: rt_yield(tag, payload, resume_state, regs_ptr, num_regs, func_idx) -> ()
        types.ty().function(
            [
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [],
        );
        // Type 11: rt_get_resume_value() -> (tag, payload)
        types.ty().function([], [ValType::I64, ValType::I64]);
        // Type 12: rt_load_saved_reg(index) -> (tag, payload)
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        module.section(&types);

        // Import section (function indices 0–10)
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

        // Function section: entry (type 0) + closures (type 5 each)
        let mut functions = FunctionSection::new();
        functions.function(0); // entry function
        for _ in 0..num_closures {
            functions.function(5); // closure function
        }
        module.section(&functions);

        // Table section: funcref table for closure indirect calls
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

        // Element section: initialize table with closure function refs
        if num_closures > 0 {
            let mut elements = ElementSection::new();
            // Table index 0, offset 0, function indices starting at FN_ENTRY + 1
            let func_indices: Vec<u32> = (0..num_closures).map(|i| FN_ENTRY + 1 + i).collect();
            elements.active(
                Some(0),
                &ConstExpr::i32_const(0),
                Elements::Functions(func_indices.into()),
            );
            module.section(&elements);
        }

        // Code section: emit entry function + all closure functions
        let mut code = CodeSection::new();

        // Entry function body
        let entry_body = self.emit_function(func);
        code.function(&entry_body);

        // Closure function bodies
        for (i, nested) in nested_funcs.iter().enumerate() {
            self.current_table_idx = i as u32;
            let closure_body = self.emit_closure_function(nested);
            code.function(&closure_body);
        }

        module.section(&code);

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
        }
    }

    /// Emit a WASM module containing a single closure function.
    ///
    /// The function is placed at FN_ENTRY and in table slot 0.
    /// Module structure mirrors emit_module but with exactly one function.
    fn emit_single_closure_module(&mut self, func: &LirFunction) -> EmitResult {
        let mut module = Module::new();

        // Type section (same as full module)
        let mut types = TypeSection::new();
        types
            .ty()
            .function([], [ValType::I64, ValType::I64, ValType::I32]);
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
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
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        // Type 5: closure function
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64, ValType::I32],
        );
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32],
            [ValType::I64, ValType::I64],
        );
        types.ty().function([ValType::I32, ValType::I32], []);
        types.ty().function([], []);
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
        types.ty().function(
            [
                ValType::I64,
                ValType::I64,
                ValType::I32,
                ValType::I32,
                ValType::I32,
                ValType::I32,
            ],
            [],
        );
        types.ty().function([], [ValType::I64, ValType::I64]);
        types
            .ty()
            .function([ValType::I32], [ValType::I64, ValType::I64]);
        module.section(&types);

        // Import section (same as full module)
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

        // Function section: one closure function (type 5)
        let mut functions = FunctionSection::new();
        functions.function(5);
        module.section(&functions);

        // Table section: 1-entry funcref table (for potential self-calls via rt_call)
        let mut tables = TableSection::new();
        tables.table(TableType {
            element_type: RefType::FUNCREF,
            minimum: 1,
            maximum: Some(1),
            shared: false,
            table64: false,
        });
        module.section(&tables);

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
        exports.export("__elle_closure", ExportKind::Func, FN_ENTRY);
        exports.export("__elle_memory", ExportKind::Memory, 0);
        exports.export("__elle_table", ExportKind::Table, 0);
        module.section(&exports);

        // Element section: function at table index 0
        let mut elements = ElementSection::new();
        elements.active(
            Some(0),
            &ConstExpr::i32_const(0),
            Elements::Functions(vec![FN_ENTRY].into()),
        );
        module.section(&elements);

        // Code section: the closure function
        let mut code = CodeSection::new();
        self.current_table_idx = 0;
        let closure_body = self.emit_closure_function(func);
        code.function(&closure_body);

        module.section(&code);

        EmitResult {
            wasm_bytes: module.finish(),
            const_pool: std::mem::take(&mut self.const_pool),
        }
    }

    fn emit_function(&mut self, func: &LirFunction) -> Function {
        self.label_to_idx.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        // Run register allocation to compact virtual regs → reusable slots.
        // Pin local-slot registers (0..num_locals) since LoadLocal/StoreLocal
        // maps slot N to Reg(N) via copy_reg in the entry function.
        let alloc = super::regalloc::allocate(func, func.num_locals as u32);
        let n = alloc.max_slots;
        self.reg_to_slot = alloc.reg_to_slot;
        self.num_regs = n;
        self.local_offset = 0;
        self.is_closure = false;
        // Locals: tags [0,N), payloads [N,2N), env_ptr (2N), signal/state (2N+1)
        self.signal_local = n * 2 + 1;

        let mut f = Function::new([
            (n, ValType::I64), // tags
            (n, ValType::I64), // payloads
            (1, ValType::I32), // env_ptr
            (1, ValType::I32), // signal/state scratch
        ]);

        self.emit_cfg(&mut f, func);

        f.instruction(&Instruction::End);
        f
    }

    /// Emit a control flow graph using loop + br_table dispatch.
    ///
    /// Each LIR basic block becomes a case in a br_table. A `$state` local
    /// tracks which block to execute next. Return terminators break out of
    /// the loop. Jump and Branch terminators set `$state` and continue.
    ///
    /// This handles any CFG topology (including cond's deeply nested
    /// if/else patterns) without needing merge-point analysis.
    fn emit_cfg(&mut self, f: &mut Function, func: &LirFunction) {
        let num_blocks = func.blocks.len();
        if num_blocks == 0 {
            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
            f.instruction(&Instruction::I64Const(0));
            f.instruction(&Instruction::I32Const(0)); // status: normal return
            return;
        }

        // Single block: no loop needed (unless suspending with call continuations)
        if num_blocks == 1 && self.call_continuations.is_empty() {
            let block = &func.blocks[0];
            for spanned in &block.instructions {
                self.emit_instr(f, &spanned.instr);
                // Yield-through check for calls in suspending functions
                if self.may_suspend {
                    match &spanned.instr {
                        LirInstr::Call { dst, .. } | LirInstr::CallArrayMut { dst, .. } => {
                            let resume_state = self.next_resume_state;
                            self.next_resume_state += 1;
                            self.emit_yield_through_check(f, *dst, resume_state);
                        }
                        _ => {}
                    }
                }
            }
            self.emit_terminator_return(f, &block.terminator.terminator);
            return;
        }

        // State local: reuse signal_local as state (i32).
        // We'll use a separate local for state to avoid conflicts.
        // State local is at self.local_offset + 2*num_regs + 1 for closures,
        // or 2*num_regs + 1 for entry. But signal_local is already there.
        // Add another i32 local for state... actually signal_local IS an i32.
        // Let's just use signal_local as the state variable (it's only used
        // transiently during call emission, and the loop dispatch doesn't
        // overlap with those uses).
        let state_local = self.signal_local;

        // Resume prologue: if ctx != 0, restore saved state and jump to resume target
        if self.may_suspend && self.is_closure && !self.resume_states.is_empty() {
            self.emit_resume_prologue(f, state_local);
        } else {
            // Initialize state to entry block index
            let entry_idx = self.label_to_idx[&func.entry] as i32;
            f.instruction(&Instruction::I32Const(entry_idx));
            f.instruction(&Instruction::LocalSet(state_local));
        }

        // Outer block for return (br 0 from inside loop = break to after block)
        // loop $dispatch
        //   block $b0
        //     block $b1
        //       ...
        //         block $bN
        //           br_table $b0 $b1 ... $bN (local.get $state)
        //         end  ;; $bN
        //       ...
        //     end  ;; $b1
        //   end  ;; $b0
        //   ;; block 0 code here (br_table jumps to $b0 which is the outermost block
        //   ;;   so after its `end` we fall through to block 0's code)
        //   ... set state, br $dispatch ...
        // end  ;; loop

        // Structure: block { loop { block*N { br_table } code_N; code_N-1; ... code_0; br loop } }

        // The br_table maps index i to block depth. Block depth 0 = innermost,
        // N-1 = outermost of the nested blocks. After landing at block i's end,
        // execution falls through to block i's code.

        // Total blocks = real LIR blocks + virtual resume blocks (for call sites)
        let num_virtual = self.call_continuations.len();
        let total_blocks = num_blocks + num_virtual;

        // Loop for dispatch. All exits use `return`.
        f.instruction(&Instruction::Loop(BlockType::Empty));

        // Nested blocks for br_table targets (innermost = highest index)
        for _ in 0..total_blocks {
            f.instruction(&Instruction::Block(BlockType::Empty));
        }

        // br_table dispatch
        f.instruction(&Instruction::LocalGet(state_local));
        let targets: Vec<u32> = (0..total_blocks as u32)
            .map(|i| total_blocks as u32 - 1 - i)
            .collect();
        let default = if targets.is_empty() { 0 } else { targets[0] };
        f.instruction(&Instruction::BrTable(targets.into(), default));

        // Emit virtual resume blocks first (they have the highest indices)
        // Virtual blocks are numbered num_blocks..num_blocks+num_virtual-1
        // In the br_table, they're innermost (emitted first in reverse)
        for virt_idx in (0..num_virtual).rev() {
            f.instruction(&Instruction::End); // close the block

            let cont = &self.call_continuations[virt_idx];
            let src_block_idx = cont.source_block_idx;
            let instr_offset = cont.instr_offset;
            let dst = cont.dst;

            // Load resume value into call dst register
            f.instruction(&Instruction::LocalGet(self.resume_tag_local));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.resume_pay_local));
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));

            // Emit remaining instructions from the source block
            let block = &func.blocks[src_block_idx];
            for spanned in &block.instructions[instr_offset..] {
                self.emit_instr(f, &spanned.instr);
            }

            // Emit the source block's terminator
            // Loop depth for virtual block virt_idx = num_blocks + virt_idx
            self.emit_block_terminator(
                f,
                &block.terminator.terminator,
                state_local,
                (num_blocks + virt_idx) as u32,
                Some(src_block_idx),
            );
        }

        // Emit real blocks in reverse order
        for block_idx in (0..num_blocks).rev() {
            f.instruction(&Instruction::End); // close the block

            let block = &func.blocks[block_idx];

            // Emit instructions (with yield-through checks for calls)
            self.emit_block_instructions(f, block_idx, func);

            // Emit terminator — loop depth for real block = block_idx
            self.emit_block_terminator(
                f,
                &block.terminator.terminator,
                state_local,
                block_idx as u32,
                Some(block_idx),
            );
        }

        f.instruction(&Instruction::End); // loop
                                          // Unreachable — all paths through the loop use `return`
        f.instruction(&Instruction::Unreachable);
    }

    /// Emit a block's instructions, with yield-through checks for calls
    /// in suspending functions.
    fn emit_block_instructions(&mut self, f: &mut Function, block_idx: usize, func: &LirFunction) {
        self.known_int.clear();
        let block = &func.blocks[block_idx];
        for (instr_idx, spanned) in block.instructions.iter().enumerate() {
            // SuspendingCall/CallArrayMut in suspending functions use the
            // yield-aware signal handler with spill/restore continuations.
            // Regular Call always uses the simple emit_call path.
            if self.may_suspend {
                match &spanned.instr {
                    LirInstr::SuspendingCall {
                        dst,
                        func: fn_reg,
                        args,
                    } => {
                        let resume_state = self
                            .call_state_map
                            .get(&(block_idx, instr_idx))
                            .copied()
                            .unwrap_or(0);
                        self.emit_call_suspending(f, *dst, *fn_reg, args, resume_state);
                        continue;
                    }
                    LirInstr::CallArrayMut {
                        dst,
                        func: fn_reg,
                        args,
                    } => {
                        let resume_state = self
                            .call_state_map
                            .get(&(block_idx, instr_idx))
                            .copied()
                            .unwrap_or(0);
                        self.emit_call_array_suspending(f, *dst, *fn_reg, *args, resume_state);
                        continue;
                    }
                    _ => {}
                }
            }
            self.emit_instr(f, &spanned.instr);
        }
    }

    /// Emit a block terminator.
    /// `loop_depth` is the br depth to reach the dispatch loop from this code position.
    /// `lir_block_idx` is the index of the LIR block (for yield state lookup).
    fn emit_block_terminator(
        &mut self,
        f: &mut Function,
        term: &Terminator,
        state_local: u32,
        loop_depth: u32,
        lir_block_idx: Option<usize>,
    ) {
        match term {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::I32Const(0)); // status: normal return
                f.instruction(&Instruction::Return);
            }
            Terminator::Jump(target) => {
                let target_idx = self.label_to_idx[target] as i32;
                f.instruction(&Instruction::I32Const(target_idx));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::Br(loop_depth));
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let then_idx = self.label_to_idx[then_label] as i32;
                let else_idx = self.label_to_idx[else_label] as i32;

                self.emit_truthiness_check(f, *cond);
                f.instruction(&Instruction::If(BlockType::Empty));
                f.instruction(&Instruction::I32Const(then_idx));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::Else);
                f.instruction(&Instruction::I32Const(else_idx));
                f.instruction(&Instruction::LocalSet(state_local));
                f.instruction(&Instruction::End);

                f.instruction(&Instruction::Br(loop_depth));
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            Terminator::Yield { value, .. } => {
                if self.may_suspend {
                    let resume_state = lir_block_idx
                        .and_then(|idx| self.yield_state_map.get(&idx).copied())
                        .unwrap_or_else(|| {
                            let s = self.next_resume_state;
                            self.next_resume_state += 1;
                            s
                        });

                    let total_saved = self.num_regs + self.num_stack_locals;
                    self.emit_spill_all(f);

                    f.instruction(&Instruction::LocalGet(self.tag_local(*value)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*value)));
                    f.instruction(&Instruction::I32Const(resume_state as i32));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(total_saved as i32));
                    f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
                    f.instruction(&Instruction::Call(FN_RT_YIELD));

                    f.instruction(&Instruction::LocalGet(self.tag_local(*value)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*value)));
                    f.instruction(&Instruction::I32Const(resume_state as i32));
                    f.instruction(&Instruction::Return);
                } else {
                    f.instruction(&Instruction::Unreachable);
                }
            }
        }
    }

    /// Emit a function call in a suspending function.
    /// Like emit_call, but checks for SIG_YIELD before the general signal return.
    fn emit_call_suspending(
        &self,
        f: &mut Function,
        dst: Reg,
        func: Reg,
        args: &[Reg],
        resume_state: u32,
    ) {
        // Write args to linear memory
        for (i, arg) in args.iter().enumerate() {
            self.write_val_to_mem(f, *arg, i);
        }

        // Call rt_call
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(args.len() as i32));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));

        // Pop results: (tag, payload, signal) — signal on top
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));

        // Check SIG_YIELD first (bit 1 = value 2)
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Const(2)); // SIG_YIELD
        f.instruction(&Instruction::I32And);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            let total_saved = self.num_regs + self.num_stack_locals;
            self.emit_spill_all(f);

            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::I32Const(total_saved as i32));
            f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
            f.instruction(&Instruction::Call(FN_RT_YIELD));

            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);

        // Check other signals (error etc.)
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I32Const(0)); // status: normal return
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit CallArrayMut in a suspending function.
    fn emit_call_array_suspending(
        &self,
        f: &mut Function,
        dst: Reg,
        func: Reg,
        args_array: Reg,
        resume_state: u32,
    ) {
        self.write_val_to_mem(f, func, 0);
        self.write_val_to_mem(f, args_array, 1);
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(-1));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));

        // Same signal handling as emit_call_suspending
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));

        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::I32And);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            let total_saved = self.num_regs + self.num_stack_locals;
            self.emit_spill_all(f);
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::I32Const(total_saved as i32));
            f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
            f.instruction(&Instruction::Call(FN_RT_YIELD));
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);

        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit the SIG_YIELD check after a call in a suspending function.
    /// If the callee yielded, spill caller state and return suspended.
    fn emit_yield_through_check(&self, f: &mut Function, dst: Reg, resume_state: u32) {
        let total_saved = self.num_regs + self.num_stack_locals;

        // The signal is already in memory[0..4] (written by store_result_with_signal)
        // and the dst register holds the yielded value.
        // Read signal from memory.
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::I32Load(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::I32Const(2)); // SIG_YIELD bit
        f.instruction(&Instruction::I32And);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            // Clear signal word (so it doesn't affect future calls)
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I32Const(0));
            f.instruction(&Instruction::I32Store(MemArg {
                offset: 0,
                align: 2,
                memory_index: 0,
            }));

            // Spill all registers + local slots
            self.emit_spill_all(f);

            // Call rt_yield with the callee's yielded value
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::I32Const(total_saved as i32));
            f.instruction(&Instruction::I32Const(self.current_table_idx as i32));
            f.instruction(&Instruction::Call(FN_RT_YIELD));

            // Return suspended
            f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
            f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
            f.instruction(&Instruction::I32Const(resume_state as i32));
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);
    }

    /// Emit a terminator that produces the function's return values.
    /// Used for single-block functions that don't need the loop dispatch.
    fn emit_terminator_return(&self, f: &mut Function, term: &Terminator) {
        match term {
            Terminator::Return(reg) => {
                f.instruction(&Instruction::LocalGet(self.tag_local(*reg)));
                f.instruction(&Instruction::LocalGet(self.pay_local(*reg)));
                f.instruction(&Instruction::I32Const(0)); // status: normal return
            }
            Terminator::Unreachable => {
                f.instruction(&Instruction::Unreachable);
            }
            _ => {
                // Shouldn't happen for single-block functions
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
                match value {
                    LirConst::Int(_) => {
                        self.known_int.insert(*dst);
                    }
                    _ => {
                        self.known_int.remove(dst);
                    }
                }
            }
            LirInstr::ValueConst { dst, value } => {
                self.emit_value_const(f, *dst, *value);
                self.known_int.remove(dst);
            }
            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let both_int = self.known_int.contains(lhs) && self.known_int.contains(rhs);
                self.emit_binop(f, *dst, *op, *lhs, *rhs, both_int);
                // Propagate: int binop on two ints produces int; bitwise always int
                let is_bitwise = matches!(
                    op,
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                );
                if both_int || is_bitwise {
                    self.known_int.insert(*dst);
                } else {
                    self.known_int.remove(dst);
                }
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
                if self.is_closure {
                    // Read from dedicated local-slot WASM locals
                    f.instruction(&Instruction::LocalGet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.local_slot_pay(*slot)));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    // Entry function: locals share the register space
                    let src = Reg(*slot as u32);
                    self.copy_reg(f, src, *dst);
                }
            }
            LirInstr::StoreLocal { slot, src } => {
                if self.is_closure {
                    // Write to dedicated local-slot WASM locals
                    f.instruction(&Instruction::LocalGet(self.tag_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_tag(*slot)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*src)));
                    f.instruction(&Instruction::LocalSet(self.local_slot_pay(*slot)));
                } else {
                    // Entry function: locals share the register space
                    let dst = Reg(*slot as u32);
                    self.copy_reg(f, *src, dst);
                }
            }
            LirInstr::LoadCapture { dst, index } => {
                // Load from closure env, auto-unwrap LBox if needed.
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
                f.instruction(&Instruction::Drop); // signal (store_lbox can't fail)
                f.instruction(&Instruction::Drop); // payload
                f.instruction(&Instruction::Drop); // tag
            }
            LirInstr::Call { dst, func, args } => {
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::SuspendingCall { dst, func, args } => {
                // In a non-suspending context (entry function), treat the same
                // as a regular call — propagate signals to the host.
                self.emit_call(f, *dst, *func, args);
            }
            LirInstr::TailCall { func, args } => {
                if !self.is_closure {
                    // Entry function: no env_ptr, use regular call + return
                    let dst = Reg(0);
                    self.emit_call(f, dst, *func, args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::I32Const(0)); // status: normal return
                    f.instruction(&Instruction::Return);
                } else {
                    // Closure: write args, call rt_prepare_tail_call, dispatch
                    for (i, arg) in args.iter().enumerate() {
                        self.write_val_to_mem(f, *arg, i);
                    }
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(args.len() as i32));
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
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
                if !self.is_closure {
                    // Entry function: use regular call + return
                    let dst = Reg(0);
                    self.emit_call_array(f, dst, *func, *args);
                    f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
                    f.instruction(&Instruction::I32Const(0)); // status: normal return
                    f.instruction(&Instruction::Return);
                } else {
                    // Closure: write array arg, call rt_prepare_tail_call with nargs=-1
                    self.write_val_to_mem(f, *args, 1);
                    f.instruction(&Instruction::LocalGet(self.tag_local(*func)));
                    f.instruction(&Instruction::LocalGet(self.pay_local(*func)));
                    f.instruction(&Instruction::I32Const(ARGS_BASE));
                    f.instruction(&Instruction::I32Const(-1)); // nargs=-1: unpack array
                    f.instruction(&Instruction::LocalGet(self.env_local()));
                    f.instruction(&Instruction::Call(FN_RT_PREPARE_TAIL_CALL));
                    self.emit_tail_call_dispatch(f);
                }
            }
            LirInstr::Eval { dst, expr, env } => {
                // Eval not supported in WASM backend — emit unreachable.
                // The analyzer should prevent eval from reaching emission
                // in most cases; this guards against edge cases.
                let _ = (dst, expr, env);
                f.instruction(&Instruction::Unreachable);
            }
            LirInstr::LoadResumeValue { dst } => {
                if self.may_suspend {
                    // Load from resume locals (set in resume prologue)
                    f.instruction(&Instruction::LocalGet(self.resume_tag_local));
                    f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                    f.instruction(&Instruction::LocalGet(self.resume_pay_local));
                    f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                } else {
                    f.instruction(&Instruction::Unreachable);
                }
            }
            LirInstr::MakeClosure {
                dst,
                func: nested,
                captures,
            } => {
                let table_idx = self
                    .closure_table_idx
                    .get(&(&**nested as *const LirFunction))
                    .copied()
                    .expect("MakeClosure: nested function not found in table");

                // Write captures to linear memory at ARGS_BASE
                for (i, cap) in captures.iter().enumerate() {
                    self.write_val_to_mem(f, *cap, i);
                }

                // Write metadata to linear memory after captures.
                // Metadata layout (starting at ARGS_BASE + captures.len()*16):
                //   [0]: num_captures (i64)
                //   [1]: num_params (i64)
                //   [2]: num_locals (i64)
                //   [3]: arity_kind (i64: 0=Exact, 1=AtLeast, 2=Range)
                //   [4]: arity_count (i64)
                //   [5]: lbox_params_mask (i64)
                //   [6]: lbox_locals_mask (i64)
                //   [7]: signal_bits (i64)
                let meta_base = ARGS_BASE + (captures.len() as i32) * 16;
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
                    nested.lbox_params_mask as i64,
                    nested.lbox_locals_mask as i64,
                    nested.signal.bits.0 as i64,
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

                // Call rt_make_closure(table_idx, captures_ptr, metadata_ptr)
                f.instruction(&Instruction::I32Const(table_idx as i32));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(meta_base));
                f.instruction(&Instruction::Call(FN_RT_MAKE_CLOSURE));
                // Returns (tag, payload)
                f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
            }
            LirInstr::PushParamFrame { pairs } => {
                // Write (param_tag, param_payload, value_tag, value_payload)
                // for each pair to ARGS_BASE, then call rt_push_param.
                for (i, (param_reg, val_reg)) in pairs.iter().enumerate() {
                    self.write_val_to_mem_offset(f, *param_reg, ARGS_BASE + (i as i32) * 32);
                    self.write_val_to_mem_offset(f, *val_reg, ARGS_BASE + (i as i32) * 32 + 16);
                }
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(pairs.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_PUSH_PARAM));
            }
            LirInstr::PopParamFrame => {
                f.instruction(&Instruction::Call(FN_RT_POP_PARAM));
            }
            LirInstr::CheckSignalBound { .. } => {
                // Compile-time validation — no-op at runtime.
            }
            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                // Write src struct as arg[0], exclude keys as arg[1..n].
                // Keys are loaded via const pool (dst is scratch — overwritten by result).
                self.write_val_to_mem(f, *src, 0);
                for (i, key) in exclude_keys.iter().enumerate() {
                    match key {
                        LirConst::Keyword(name) => {
                            self.emit_const_pool_load(f, *dst, Value::keyword(name));
                        }
                        LirConst::Symbol(id) => {
                            self.emit_const_pool_load(f, *dst, Value::symbol(id.0));
                        }
                        _ => {
                            f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                            f.instruction(&Instruction::LocalSet(self.tag_local(*dst)));
                            f.instruction(&Instruction::I64Const(0));
                            f.instruction(&Instruction::LocalSet(self.pay_local(*dst)));
                        }
                    }
                    self.write_val_to_mem(f, *dst, i + 1);
                }
                f.instruction(&Instruction::I32Const(OP_STRUCT_REST));
                f.instruction(&Instruction::I32Const(ARGS_BASE));
                f.instruction(&Instruction::I32Const(1 + exclude_keys.len() as i32));
                f.instruction(&Instruction::Call(FN_RT_DATA_OP));
                self.store_result_with_signal(f, *dst);
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
        self.store_result_with_signal(f, dst);
    }

    /// Like emit_call but drops the signal instead of checking it.
    /// Used for SuspendingCall in non-suspending contexts (entry function)
    /// where I/O signals should be ignored — the I/O already completed on the host.
    #[allow(dead_code)]
    fn emit_call_ignore_signal(&self, f: &mut Function, dst: Reg, func: Reg, args: &[Reg]) {
        for (i, arg) in args.iter().enumerate() {
            self.write_val_to_mem(f, *arg, i);
        }
        f.instruction(&Instruction::LocalGet(self.tag_local(func)));
        f.instruction(&Instruction::LocalGet(self.pay_local(func)));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(args.len() as i32));
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::Call(FN_RT_CALL));
        // Drop signal, keep tag+payload
        f.instruction(&Instruction::Drop);
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
        self.store_result_with_signal(f, dst);
    }

    // --- Data operation helpers ---

    /// Store result from a host call that returns (tag, payload, signal).
    /// Signal is on top of stack. On non-zero signal, writes the signal
    /// to memory at byte 0 (so the host can read it after the call) and
    /// returns the error value.
    fn store_result_with_signal(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::LocalSet(self.signal_local));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::If(BlockType::Empty));
        // Write signal to memory[0..4] for host to read after return
        f.instruction(&Instruction::I32Const(0));
        f.instruction(&Instruction::LocalGet(self.signal_local));
        f.instruction(&Instruction::I32Store(MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        f.instruction(&Instruction::LocalGet(self.tag_local(dst)));
        f.instruction(&Instruction::LocalGet(self.pay_local(dst)));
        f.instruction(&Instruction::I32Const(0)); // status: normal return (error propagated via signal memory)
        f.instruction(&Instruction::Return);
        f.instruction(&Instruction::End);
    }

    /// Emit the dispatch logic after `rt_prepare_tail_call` returns.
    ///
    /// Stack on entry: `(env_ptr: i32, table_idx: i32, is_wasm: i32, tag: i64, payload: i64, signal: i32)`
    /// If is_wasm: `return_call_indirect` replaces the current frame.
    /// Otherwise: return (tag, payload) — the NativeFn/Parameter result.
    fn emit_tail_call_dispatch(&self, f: &mut Function) {
        // Temp locals for the 6 return values
        let tc_signal = self.signal_local;
        let tc_payload = self.pay_local(Reg(0));
        let tc_tag = self.tag_local(Reg(0));
        let tc_is_wasm = self.signal_local + 1;
        let tc_table_idx = self.signal_local + 2;
        let tc_env_ptr = self.signal_local + 3;

        // Pop results (WASM stack order: last return value on top)
        f.instruction(&Instruction::LocalSet(tc_signal)); // i32
        f.instruction(&Instruction::LocalSet(tc_payload)); // i64
        f.instruction(&Instruction::LocalSet(tc_tag)); // i64
        f.instruction(&Instruction::LocalSet(tc_is_wasm)); // i32
        f.instruction(&Instruction::LocalSet(tc_table_idx)); // i32
        f.instruction(&Instruction::LocalSet(tc_env_ptr)); // i32

        // Dispatch based on is_wasm
        f.instruction(&Instruction::LocalGet(tc_is_wasm));
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            // WASM closure: return_call_indirect replaces this frame
            f.instruction(&Instruction::LocalGet(tc_env_ptr));
            f.instruction(&Instruction::I32Const(0)); // args_ptr unused
            f.instruction(&Instruction::I32Const(0)); // nargs unused
            f.instruction(&Instruction::I32Const(0)); // ctx
            f.instruction(&Instruction::LocalGet(tc_table_idx));
            f.instruction(&Instruction::ReturnCallIndirect {
                type_index: 5,
                table_index: 0,
            });
        }
        f.instruction(&Instruction::Else);
        {
            // NativeFn/Parameter result: return (tag, payload, 0)
            f.instruction(&Instruction::LocalGet(tc_tag));
            f.instruction(&Instruction::LocalGet(tc_payload));
            f.instruction(&Instruction::I32Const(0)); // status: normal return
            f.instruction(&Instruction::Return);
        }
        f.instruction(&Instruction::End);
    }

    /// 1-arg data op: write arg to memory, call rt_data_op, store result.
    fn emit_data_op1(&self, f: &mut Function, dst: Reg, op: i32, src: Reg) {
        self.write_val_to_mem(f, src, 0);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(1));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
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
        self.store_result_with_signal(f, dst);
    }

    /// 2-arg data op.
    fn emit_data_op2(&self, f: &mut Function, dst: Reg, op: i32, a: Reg, b: Reg) {
        self.write_val_to_mem(f, a, 0);
        self.write_val_to_mem(f, b, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
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
        self.store_result_with_signal(f, dst);
    }

    /// Struct get with a constant key (keyword or symbol from LirConst).
    fn emit_struct_get(&mut self, f: &mut Function, dst: Reg, op: i32, src: Reg, key: &LirConst) {
        self.write_val_to_mem(f, src, 0);
        // Load key via const pool into dst (scratch — overwritten by result),
        // then write to memory slot 1.
        match key {
            LirConst::Keyword(name) => self.emit_const_pool_load(f, dst, Value::keyword(name)),
            LirConst::Symbol(id) => self.emit_const_pool_load(f, dst, Value::symbol(id.0)),
            _ => {
                f.instruction(&Instruction::I64Const(TAG_NIL as i64));
                f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
                f.instruction(&Instruction::I64Const(0));
                f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
            }
        }
        self.write_val_to_mem(f, dst, 1);
        f.instruction(&Instruction::I32Const(op));
        f.instruction(&Instruction::I32Const(ARGS_BASE));
        f.instruction(&Instruction::I32Const(2));
        f.instruction(&Instruction::Call(FN_RT_DATA_OP));
        self.store_result_with_signal(f, dst);
    }

    /// Write a register's value to linear memory at ARGS_BASE + slot*16.
    fn write_val_to_mem(&self, f: &mut Function, reg: Reg, slot: usize) {
        self.write_val_to_mem_offset(f, reg, ARGS_BASE + (slot as i32) * 16);
    }

    /// Write a register's value to linear memory at an absolute byte offset.
    fn write_val_to_mem_offset(&self, f: &mut Function, reg: Reg, base: i32) {
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

    /// Spill all registers + local slots to linear memory at ARGS_BASE.
    /// Each slot is 16 bytes (tag: i64, payload: i64).
    /// Layout: [reg0_tag, reg0_pay, reg1_tag, reg1_pay, ..., local0_tag, local0_pay, ...]
    fn emit_spill_all(&self, f: &mut Function) {
        // Spill registers (physical slots, not virtual regs)
        for i in 0..self.num_regs {
            let offset = (i * 16) as u64;
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.tag_phys(i)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.pay_phys(i)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }
        // Spill local slots
        for i in 0..self.num_stack_locals {
            let offset = ((self.num_regs + i) * 16) as u64;
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.local_slot_tag(i as u16)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset,
                align: 3,
                memory_index: 0,
            }));
            f.instruction(&Instruction::I32Const(ARGS_BASE));
            f.instruction(&Instruction::LocalGet(self.local_slot_pay(i as u16)));
            f.instruction(&Instruction::I64Store(MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
        }
    }

    /// Restore all registers + local slots from saved data via rt_load_saved_reg.
    fn emit_restore_all(&self, f: &mut Function, num_saved: u32) {
        let num_regs = self.num_regs.min(num_saved);
        let num_locals = (num_saved - num_regs).min(self.num_stack_locals);

        // Restore registers (physical slots)
        for i in 0..num_regs {
            f.instruction(&Instruction::I32Const(i as i32));
            f.instruction(&Instruction::Call(FN_RT_LOAD_SAVED_REG));
            // Stack: [tag, payload]
            f.instruction(&Instruction::LocalSet(self.pay_phys(i)));
            f.instruction(&Instruction::LocalSet(self.tag_phys(i)));
        }
        // Restore local slots
        for i in 0..num_locals {
            f.instruction(&Instruction::I32Const((self.num_regs + i) as i32));
            f.instruction(&Instruction::Call(FN_RT_LOAD_SAVED_REG));
            f.instruction(&Instruction::LocalSet(self.local_slot_pay(i as u16)));
            f.instruction(&Instruction::LocalSet(self.local_slot_tag(i as u16)));
        }
    }

    /// Emit the resume prologue for suspending closures.
    ///
    /// When ctx != 0, we're resuming a previously suspended invocation:
    /// 1. Load resume value from host via rt_get_resume_value
    /// 2. Dispatch on ctx to the right restore block via br_table
    /// 3. Each restore block loads saved regs, sets state to target block
    ///
    /// When ctx == 0, fall through to normal entry.
    fn emit_resume_prologue(&self, f: &mut Function, state_local: u32) {
        let entry_idx = self.label_to_idx.values().min().copied().unwrap_or(0) as i32;

        // Check ctx (param 3)
        f.instruction(&Instruction::LocalGet(3)); // ctx
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            // Resuming: load resume value
            f.instruction(&Instruction::Call(FN_RT_GET_RESUME_VALUE));
            // Stack: [tag, payload]
            f.instruction(&Instruction::LocalSet(self.resume_pay_local));
            f.instruction(&Instruction::LocalSet(self.resume_tag_local));

            // Build nested blocks for br_table dispatch.
            // Wrap everything in an outer block so restore blocks can br to exit.
            let num_states = self.resume_states.len();
            f.instruction(&Instruction::Block(BlockType::Empty)); // $exit_block
            for _ in 0..num_states {
                f.instruction(&Instruction::Block(BlockType::Empty));
            }

            // br_table: (ctx - 1) → jump to restore block
            f.instruction(&Instruction::LocalGet(3)); // ctx
            f.instruction(&Instruction::I32Const(1));
            f.instruction(&Instruction::I32Sub);
            // Depths: state i → num_states - 1 - i (to land at restore block i)
            // But shifted by 1 because of $exit_block
            let targets: Vec<u32> = (0..num_states as u32)
                .map(|i| num_states as u32 - 1 - i)
                .collect();
            let default = if targets.is_empty() { 0 } else { targets[0] };
            f.instruction(&Instruction::BrTable(targets.into(), default));

            // Emit restore blocks (innermost end first)
            for idx in (0..num_states).rev() {
                f.instruction(&Instruction::End); // close dispatch block

                let info = &self.resume_states[idx];
                self.emit_restore_all(f, info.num_saved);
                f.instruction(&Instruction::I32Const(info.target_block_idx));
                f.instruction(&Instruction::LocalSet(state_local));
                // Branch to $exit_block to skip remaining restore blocks.
                // From here, depth 0..idx-1 are the remaining dispatch blocks,
                // depth idx is $exit_block.
                f.instruction(&Instruction::Br(idx as u32));
            }
            f.instruction(&Instruction::End); // $exit_block
        }
        f.instruction(&Instruction::Else);
        {
            // Normal entry: state = entry block
            f.instruction(&Instruction::I32Const(entry_idx));
            f.instruction(&Instruction::LocalSet(state_local));
        }
        f.instruction(&Instruction::End);
    }

    /// Pre-scan a LirFunction to build the resume_states table.
    /// Must be called before emit_cfg for suspending closures.
    fn pre_scan_resume_states(&mut self, func: &LirFunction) {
        self.resume_states.clear();
        self.call_continuations.clear();
        self.yield_state_map.clear();
        self.call_state_map.clear();
        self.next_resume_state = 1;
        let total_saved = self.num_regs + self.num_stack_locals;
        let num_real_blocks = func.blocks.len();

        for block in &func.blocks {
            let block_idx = self.label_to_idx[&block.label];

            // Yield terminators
            if let Terminator::Yield { resume_label, .. } = &block.terminator.terminator {
                let state_id = self.next_resume_state;
                self.next_resume_state += 1;
                let target_block_idx = self.label_to_idx[resume_label] as i32;
                self.resume_states.push(ResumeStateInfo {
                    state_id,
                    target_block_idx,
                    num_saved: total_saved,
                });
                self.yield_state_map.insert(block_idx, state_id);
            }

            // Call sites: only SuspendingCall/CallArrayMut are yield-through points.
            // Regular Call is known to not suspend, so no continuation needed.
            for (instr_idx, spanned) in block.instructions.iter().enumerate() {
                let dst = match &spanned.instr {
                    LirInstr::SuspendingCall { dst, .. } => Some(*dst),
                    LirInstr::CallArrayMut { dst, .. } => Some(*dst),
                    _ => None,
                };
                if let Some(dst) = dst {
                    let state_id = self.next_resume_state;
                    self.next_resume_state += 1;
                    let virtual_idx = (num_real_blocks + self.call_continuations.len()) as i32;
                    self.resume_states.push(ResumeStateInfo {
                        state_id,
                        target_block_idx: virtual_idx,
                        num_saved: total_saved,
                    });
                    self.call_state_map.insert((block_idx, instr_idx), state_id);
                    self.call_continuations.push(CallSiteContinuation {
                        dst,
                        source_block_idx: block_idx,
                        instr_offset: instr_idx + 1,
                    });
                }
            }
        }
    }

    /// Add a value to the constant pool and emit rt_load_const into dst.
    fn emit_const_pool_load(&mut self, f: &mut Function, dst: Reg, value: Value) {
        let idx = self.const_pool.len() as i32;
        self.const_pool.push(value);
        f.instruction(&Instruction::I32Const(idx));
        f.instruction(&Instruction::Call(FN_RT_LOAD_CONST));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
    }

    fn emit_const(&mut self, f: &mut Function, dst: Reg, value: &LirConst) {
        match value {
            LirConst::String(s) => {
                self.emit_const_pool_load(f, dst, Value::string(s.clone()));
            }
            // Symbols and keywords have runtime-allocated IDs — route through
            // the constant pool so WASM bytes are deterministic.
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
                    LirConst::Symbol(_) | LirConst::Keyword(_) | LirConst::String(_) => {
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

    fn emit_binop(
        &self,
        f: &mut Function,
        dst: Reg,
        op: BinOp,
        lhs: Reg,
        rhs: Reg,
        both_int: bool,
    ) {
        // Integer-only fast path: both operands statically known to be int,
        // or bitwise ops which are always int.
        if both_int
            || matches!(
                op,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            )
        {
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
            return;
        }

        // Arithmetic ops: check if either operand is float.
        // if lhs.tag == TAG_FLOAT || rhs.tag == TAG_FLOAT → float path
        // else → integer path
        f.instruction(&Instruction::LocalGet(self.tag_local(lhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::LocalGet(self.tag_local(rhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::I32Or);
        f.instruction(&Instruction::If(BlockType::Empty));
        {
            // Float path: reinterpret payloads as f64, promote ints
            f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
            f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
            // Load lhs as f64 (promote int if needed)
            self.emit_to_f64(f, lhs);
            // Load rhs as f64
            self.emit_to_f64(f, rhs);
            match op {
                BinOp::Add => {
                    f.instruction(&Instruction::F64Add);
                }
                BinOp::Sub => {
                    f.instruction(&Instruction::F64Sub);
                }
                BinOp::Mul => {
                    f.instruction(&Instruction::F64Mul);
                }
                BinOp::Div => {
                    f.instruction(&Instruction::F64Div);
                }
                BinOp::Rem => {
                    // f64 remainder: a - floor(a/b) * b
                    f.instruction(&Instruction::Drop); // drop the two f64s from emit_to_f64
                    f.instruction(&Instruction::Drop);
                    self.emit_to_f64(f, lhs); // a
                    self.emit_to_f64(f, lhs); // a (for a/b)
                    self.emit_to_f64(f, rhs); // b
                    f.instruction(&Instruction::F64Div); // a/b
                    f.instruction(&Instruction::F64Floor); // floor(a/b)
                    self.emit_to_f64(f, rhs); // b
                    f.instruction(&Instruction::F64Mul); // floor(a/b)*b
                    f.instruction(&Instruction::F64Sub); // a - floor(a/b)*b
                }
                _ => unreachable!(),
            }
            // Reinterpret f64 result bits as i64 for payload
            f.instruction(&Instruction::I64ReinterpretF64);
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        }
        f.instruction(&Instruction::Else);
        {
            // Integer path
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
                _ => unreachable!(),
            };
            f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
        }
        f.instruction(&Instruction::End);
    }

    /// Load a register's payload as f64, promoting int to float if needed.
    fn emit_to_f64(&self, f: &mut Function, reg: Reg) {
        f.instruction(&Instruction::LocalGet(self.tag_local(reg)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::If(BlockType::Result(ValType::F64)));
        // Float: reinterpret bits as f64
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::F64ReinterpretI64);
        f.instruction(&Instruction::Else);
        // Int: convert i64 to f64
        f.instruction(&Instruction::LocalGet(self.pay_local(reg)));
        f.instruction(&Instruction::F64ConvertI64S);
        f.instruction(&Instruction::End);
    }

    fn emit_compare(&self, f: &mut Function, dst: Reg, op: CmpOp, lhs: Reg, rhs: Reg) {
        // Check if either operand is float
        f.instruction(&Instruction::LocalGet(self.tag_local(lhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::LocalGet(self.tag_local(rhs)));
        f.instruction(&Instruction::I64Const(TAG_FLOAT as i64));
        f.instruction(&Instruction::I64Eq);
        f.instruction(&Instruction::I32Or);
        f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
        {
            // Float comparison
            self.emit_to_f64(f, lhs);
            self.emit_to_f64(f, rhs);
            match op {
                CmpOp::Eq => f.instruction(&Instruction::F64Eq),
                CmpOp::Ne => f.instruction(&Instruction::F64Ne),
                CmpOp::Lt => f.instruction(&Instruction::F64Lt),
                CmpOp::Le => f.instruction(&Instruction::F64Le),
                CmpOp::Gt => f.instruction(&Instruction::F64Gt),
                CmpOp::Ge => f.instruction(&Instruction::F64Ge),
            };
        }
        f.instruction(&Instruction::Else);
        {
            // Integer comparison
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
        }
        f.instruction(&Instruction::End);
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

    #[allow(dead_code)]
    fn set_nil(&self, f: &mut Function, dst: Reg) {
        f.instruction(&Instruction::I64Const(TAG_NIL as i64));
        f.instruction(&Instruction::LocalSet(self.tag_local(dst)));
        f.instruction(&Instruction::I64Const(0));
        f.instruction(&Instruction::LocalSet(self.pay_local(dst)));
    }

    /// Emit a closure function body.
    ///
    /// Closure WASM type: `(env_ptr: i32, args_ptr: i32, nargs: i32, ctx: i32) -> (tag: i64, pay: i64, status: i32)`
    ///
    /// WASM local layout:
    /// - 0: env_ptr (param)
    /// - 1: args_ptr (param) — unused
    /// - 2: nargs (param) — unused
    /// - 3: ctx (param)
    /// - 4..4+N: register tags (computation intermediates)
    /// - 4+N..4+2N: register payloads
    /// - 4+2N..4+2N+M: local slot tags (non-LBox let-bound variables)
    /// - 4+2N+M..4+2N+2M: local slot payloads
    /// - 4+2N+2M: signal scratch (i32)
    /// - 4+2N+2M+1..+3: tail call temps (i32)
    ///
    /// For suspending closures, 2 extra i64 locals:
    /// - 4+2N+2M+4: resume_tag (i64)
    /// - 4+2N+2M+5: resume_pay (i64)
    ///
    /// Three address spaces:
    /// - Env (linear memory at env_ptr): captures, params, LBox locals
    /// - Local slots (WASM locals): non-LBox let-bound variables
    /// - Registers (WASM locals): computation intermediates
    fn emit_closure_function(&mut self, func: &LirFunction) -> Function {
        self.label_to_idx.clear();
        for (idx, block) in func.blocks.iter().enumerate() {
            self.label_to_idx.insert(block.label, idx);
        }

        // Run register allocation to compact virtual regs → reusable slots.
        let alloc = super::regalloc::allocate(func, 0);
        let n = alloc.max_slots;
        if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
            eprintln!(
                "[emit] closure {:?}: {} virtual regs → {} slots",
                func.name, func.num_regs, n
            );
        }
        self.reg_to_slot = alloc.reg_to_slot;
        self.num_regs = n;
        self.local_offset = 4;
        self.is_closure = true;
        self.num_stack_locals = func.num_locals as u32;
        self.may_suspend = func.signal.may_suspend();
        self.current_num_captures = func.num_captures;
        // Build LBox mask: captures have their own LBox status (unknown here,
        // so mark all captures as potentially LBox), params use lbox_params_mask,
        // locals use lbox_locals_mask.
        // Env layout: [captures(0..nc), params(nc..nc+np), locals(nc+np..)]
        let nc = func.num_captures as u64;
        // Captures: conservatively assume ALL might be LBox (set bits 0..nc)
        let capture_bits = if nc >= 64 { u64::MAX } else { (1u64 << nc) - 1 };
        // Params: shift lbox_params_mask by nc (saturate to all-set if >= 64)
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

        // Layout: params(4) + reg_tags(N) + reg_pays(N) + local_tags(M) + local_pays(M) + i32s(4)
        let m = self.num_stack_locals;
        self.signal_local = 4 + 2 * n + 2 * m;

        if self.may_suspend {
            // Extra locals for resume value
            self.resume_tag_local = 4 + 2 * n + 2 * m + 4;
            self.resume_pay_local = 4 + 2 * n + 2 * m + 5;

            // Pre-scan to build resume state table (needed by prologue).
            // This sets next_resume_state; we reset it before emit_cfg
            // so the yield emission re-assigns the same IDs.
            self.pre_scan_resume_states(func);
            self.next_resume_state = 1; // reset for emit_cfg pass

            if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
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
                (n, ValType::I64), // register tags
                (n, ValType::I64), // register payloads
                (m, ValType::I64), // local slot tags
                (m, ValType::I64), // local slot payloads
                (4, ValType::I32), // signal_scratch + 3 tail call temps
                (2, ValType::I64), // resume_tag + resume_pay
            ]);

            self.emit_cfg(&mut f, func);

            f.instruction(&Instruction::End);
            f
        } else {
            let mut f = Function::new([
                (n, ValType::I64), // register tags
                (n, ValType::I64), // register payloads
                (m, ValType::I64), // local slot tags
                (m, ValType::I64), // local slot payloads
                (4, ValType::I32), // signal_scratch + 3 tail call temps
            ]);

            self.emit_cfg(&mut f, func);

            f.instruction(&Instruction::End);
            f
        }
    }

    /// WASM local index for the env pointer.
    /// Entry function: local at 2*num_regs (declared as additional local).
    /// Closure function: param 0 (WASM local 0).
    fn env_local(&self) -> u32 {
        if self.local_offset == 0 {
            // Entry function: env_ptr is the last declared local
            self.num_regs * 2
        } else {
            // Closure function: env_ptr is param 0
            0
        }
    }

    fn tag_local(&self, reg: Reg) -> u32 {
        let slot = self.reg_to_slot.get(&reg).copied().unwrap_or(reg.0);
        self.local_offset + slot
    }

    fn pay_local(&self, reg: Reg) -> u32 {
        let slot = self.reg_to_slot.get(&reg).copied().unwrap_or(reg.0);
        self.local_offset + slot + self.num_regs
    }

    /// Direct WASM local index for a physical slot's tag (bypasses reg_to_slot).
    /// Used by spill/restore which iterate over physical slots, not virtual regs.
    fn tag_phys(&self, slot: u32) -> u32 {
        self.local_offset + slot
    }

    /// Direct WASM local index for a physical slot's payload.
    fn pay_phys(&self, slot: u32) -> u32 {
        self.local_offset + slot + self.num_regs
    }

    /// WASM local index for a stack-local variable's tag.
    /// Local slots live after the register bank: offset + 2*num_regs + slot.
    fn local_slot_tag(&self, slot: u16) -> u32 {
        self.local_offset + 2 * self.num_regs + slot as u32
    }

    /// WASM local index for a stack-local variable's payload.
    fn local_slot_pay(&self, slot: u16) -> u32 {
        self.local_offset + 2 * self.num_regs + self.num_stack_locals + slot as u32
    }
}

/// Recursively collect all nested LirFunctions from MakeClosure instructions.
fn collect_nested_functions<'a>(func: &'a LirFunction, out: &mut Vec<&'a LirFunction>) {
    for block in &func.blocks {
        for spanned in &block.instructions {
            if let LirInstr::MakeClosure { func: nested, .. } = &spanned.instr {
                out.push(nested);
                // Recurse into the nested function's own closures
                collect_nested_functions(nested, out);
            }
        }
    }
}
