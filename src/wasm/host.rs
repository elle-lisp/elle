//! Wasmtime host state and primitive dispatch.
//!
//! The host state (`ElleHost`) lives in the Wasmtime `Store` and holds:
//! - Handle table for heap objects
//! - Flattened primitive dispatch table
//! - Parameter frames for dynamic bindings
//!
//! Host functions are registered as Wasmtime imports under the "elle"
//! namespace. The main one is `call_primitive(prim_id, args_ptr, nargs, ctx)`
//! which dispatches to Elle's 331+ primitive functions.

use crate::io::request::IoRequest;
use crate::io::AnyBackend;
use crate::primitives::def::PrimitiveDef;
use crate::signals::SIG_IO;
use crate::value::fiber::SignalBits;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

use super::handle::HandleTable;

/// Bytecode + constants + child prototypes for a closure, used by spawn for
/// cross-thread execution. `rt_make_closure` reconstructs a `ClosureTemplate`
/// from these, and the OS-thread VM worker runs that template's bytecode. The
/// child prototypes are the nested-lambda blueprints the bytecode's `MakeClosure`
/// instructions index — omitting them makes the worker panic on its first
/// `MakeClosure` (src/wasm/tests.rs `wasm_full_spawn_runs_nested_closure`).
pub type ClosureBytecode = (
    std::rc::Rc<Vec<u8>>,
    std::rc::Rc<Vec<Value>>,
    std::rc::Rc<Vec<std::rc::Rc<crate::value::closure::ClosureTemplate>>>,
);

/// A pre-compiled standalone closure Module with its constant pool.
#[derive(Clone)]
pub struct PrecachedClosure {
    pub module: wasmtime::Module,
    pub const_pool: Vec<Value>,
    /// Byte offset where this closure's env stack must begin — above its widest
    /// args region so a wide call in its body cannot clobber its own env
    /// (`emit::env_stack_base_for_func`).
    pub env_stack_base: usize,
}

/// Default base address for the env stack in linear memory, used when a module's
/// widest args region fits under it. A wider module raises it per
/// `emit::env_stack_base`. Each `call_wasm_closure` allocates a region starting
/// from the store's `env_stack_ptr`.
pub const ENV_STACK_BASE: usize = 4096;

/// Saved state for a suspended WASM closure.
///
/// When a WASM closure yields (or a callee yields through it), the live
/// registers and env snapshot are saved here. On resume, the env is
/// restored to linear memory and the function is re-invoked with
/// `ctx = resume_state`.
pub struct WasmSuspensionFrame {
    /// Table index of the WASM function to re-invoke.
    pub wasm_func_idx: u32,
    /// Resume state ID (passed as `ctx` parameter on re-entry).
    pub resume_state: u32,
    /// Saved registers at the yield/call point: (tag, payload) pairs.
    pub saved_regs: Vec<(i64, i64)>,
    /// Snapshot of the env region in linear memory. Copied because the
    /// env stack allocator would reclaim the space on return.
    pub env_snapshot: Vec<u8>,
    /// Base address where env_snapshot was taken from (for restore).
    pub env_base: usize,
    /// Full signal bits at the yield point. Preserves SIG_IO and other
    /// bits so the scheduler can detect I/O requests on the fiber.
    pub signal_bits: u64,
    /// The executing closure (`SELF_SLOT`) at the yield point — (tag, payload).
    /// `rt_yield` snapshots it from linear memory; `resume_wasm_closure` writes it
    /// back before re-invoking, so a `LoadSelf` after resume names the closure that
    /// suspended (not whichever ran most recently on this store's shared memory).
    pub self_tag: i64,
    pub self_payload: i64,
    /// A child fiber this frame must RE-DRIVE before resuming its own
    /// continuation. Set when a `(fiber/resume child)` in this frame's body
    /// suspended because `child` emitted a scheduler wait/io its narrow mask does
    /// not cover: the resumer parks holding that wait and the scheduler drives it,
    /// so on resume the scheduler's value must feed a re-drive of `child` — not
    /// this frame's continuation — until `child` completes. The WASM analogue of
    /// the VM's `SuspendedFrame::FiberResume` (src/vm/fiber/trampoline.rs). Pinned
    /// by tests/elle/wasm-protect-suspend.lisp.
    pub redrive_child: Option<Value>,
}

/// Host state stored in the Wasmtime `Store<ElleHost>`.
pub struct ElleHost {
    /// Handle table for heap objects.
    pub handles: HandleTable,
    /// Flattened primitive dispatch table.
    /// Index = prim_id, value = &'static PrimitiveDef.
    pub primitives: Vec<&'static PrimitiveDef>,
    /// Constant pool for heap values referenced by the WASM module.
    /// Populated by create_store from the EmitResult.
    pub const_pool: Vec<Value>,
    /// Stack pointer for env allocation in linear memory.
    /// Each nested `call_wasm_closure` bumps this forward; on return it
    /// is restored. This prevents nested calls from overwriting each
    /// other's env regions.
    pub env_stack_ptr: usize,
    /// Parameter binding frames. Stack of frames, each frame is a vec
    /// of (parameter_id, value) pairs. PushParamFrame pushes a new
    /// frame; PopParamFrame pops.
    pub param_frames: Vec<Vec<(u32, Value)>>,
    /// Per-fiber suspension frames. Keyed by fiber ID (FiberHandle pointer
    /// address). Each fiber's frames are independent — nested fiber
    /// resumes don't interfere with the parent fiber's frames.
    ///
    /// Frames are pushed to the back (innermost first during yield-through-call)
    /// and consumed from the front (innermost first during resume). This avoids
    /// the need for reversal.
    pub suspension_frames:
        std::collections::HashMap<usize, std::collections::VecDeque<WasmSuspensionFrame>>,
    /// Stack of active fiber IDs. Pushed when entering handle_fiber_resume,
    /// popped on exit. rt_yield and rt_load_saved_reg use the top entry
    /// to find the correct fiber's frame list.
    pub fiber_id_stack: Vec<usize>,
    /// A pending child re-drive keyed by the PARENT fiber's ID. Set by
    /// `route_emit` when a `(fiber/resume child)` propagates `child`'s uncaught
    /// wait/io through the parent; consumed by `rt_yield` when it pushes the
    /// parent's continuation frame, stamping that frame's `redrive_child`. Between
    /// the two the parent's `fiber/resume` SuspendingCall is the only host call,
    /// so the mapping reaches exactly the right frame.
    pub pending_redrive: std::collections::HashMap<usize, Value>,
    /// Resume value passed by the scheduler (fiber/resume). Set before
    /// re-invoking a suspended function; consumed by rt_get_resume_value.
    pub resume_value: Option<(i64, i64)>,
    /// Mapping from const pool index → handle table index for heap values.
    /// Immediate values (tag < TAG_HEAP_START) have 0 here (unused).
    pub pool_to_handle: Vec<u64>,
    /// Bytecode for each closure, indexed by table index.
    /// Populated from EmitResult so rt_make_closure can give WASM closures
    /// valid bytecode for cross-thread execution via spawn.
    pub closure_bytecodes: Vec<ClosureBytecode>,
    /// Debug logging enabled (set once from `config.debug_wasm` at construction).
    pub debug: bool,
    /// Lazily-initialized I/O backend for inline I/O execution.
    /// Created on first use, reused for subsequent I/O operations.
    io_backend: Option<AnyBackend>,
    /// Per-closure pre-compiled standalone Modules with their const pools.
    /// Indexed by table index (= ClosureId). When set, rt_call dispatches
    /// to the pre-compiled Module instead of call_indirect on the full table.
    pub precached_closures: Vec<Option<PrecachedClosure>>,
    /// The driving VM, threaded so host primitive calls build a VM-bearing
    /// `NativeCtx` (docs/impl/region/ctx.md). Set by every Store-creation site
    /// from the VM in scope (the lazy tier's `run_wasm` pointer, `eval_wasm_raw`'s
    /// own VM, or an enclosing wasm call's host); null only on a freshly
    /// constructed host before that install, which never runs a primitive.
    pub vm: *mut crate::vm::VM,
}

impl ElleHost {
    pub fn new() -> Self {
        let primitives = build_primitive_table();
        ElleHost {
            handles: HandleTable::new(),
            primitives,
            const_pool: Vec::new(),
            env_stack_ptr: ENV_STACK_BASE,
            param_frames: Vec::new(),
            suspension_frames: std::collections::HashMap::new(),
            fiber_id_stack: Vec::new(),
            pending_redrive: std::collections::HashMap::new(),
            resume_value: None,
            pool_to_handle: Vec::new(),
            closure_bytecodes: Vec::new(),
            debug: crate::config::get().has_trace("wasm"),
            io_backend: None,
            precached_closures: Vec::new(),
            vm: std::ptr::null_mut(),
        }
    }
}

impl Default for ElleHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ElleHost {
    /// Raw pointer to the driving instance's heap (`vm.heap_ptr`). The WASM host
    /// builds every result / env / error value on the instance's own heap, reached
    /// through the threaded VM. Non-null whenever wasm executes (the VM is set at
    /// store creation; a freshly constructed host runs no code before that).
    pub(crate) fn heap_ptr(&self) -> *mut crate::value::fiberheap::FiberHeap {
        unsafe { (*self.vm).heap_ptr }
    }

    /// Get the current fiber's ID from the stack, or 0 for top-level.
    pub fn current_fiber_id(&self) -> usize {
        self.fiber_id_stack.last().copied().unwrap_or(0)
    }

    /// Push a suspension frame for the current fiber (appends to back).
    pub fn push_suspension_frame(&mut self, frame: WasmSuspensionFrame) {
        let id = self.current_fiber_id();
        self.suspension_frames
            .entry(id)
            .or_default()
            .push_back(frame);
    }

    /// Pop the front suspension frame for the current fiber (innermost first).
    pub fn pop_suspension_frame(&mut self) -> Option<WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        let frames = self.suspension_frames.get_mut(&id)?;
        let frame = frames.pop_front();
        if frames.is_empty() {
            self.suspension_frames.remove(&id);
        }
        frame
    }

    /// Get the front suspension frame for the current fiber (innermost).
    pub fn first_suspension_frame(&self) -> Option<&WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get(&id)?.front()
    }

    /// Get the front suspension frame for the current fiber (innermost, mutable).
    pub fn first_suspension_frame_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get_mut(&id)?.front_mut()
    }

    /// Get the back suspension frame for the current fiber (most recently pushed).
    /// Used by handle_wasm_result to update the frame that rt_yield just pushed.
    pub fn back_suspension_frame_mut(&mut self) -> Option<&mut WasmSuspensionFrame> {
        let id = self.current_fiber_id();
        self.suspension_frames.get_mut(&id)?.back_mut()
    }

    /// The child fiber the current fiber's FRONT frame must re-drive before
    /// resuming, if any (see `WasmSuspensionFrame::redrive_child`).
    pub fn first_frame_redrive_child(&self) -> Option<Value> {
        self.first_suspension_frame().and_then(|f| f.redrive_child)
    }

    /// Clear the FRONT frame's re-drive marker — called once the child has been
    /// driven to completion, so the frame resumes its own continuation next.
    pub fn clear_first_frame_redrive(&mut self) {
        if let Some(frame) = self.first_suspension_frame_mut() {
            frame.redrive_child = None;
        }
    }

    /// Check if the current fiber has any suspension frames.
    pub fn has_suspension_frames(&self) -> bool {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get(&id)
            .is_some_and(|f| !f.is_empty())
    }

    /// Count suspension frames for the current fiber.
    pub fn suspension_frame_count(&self) -> usize {
        let id = self.current_fiber_id();
        self.suspension_frames
            .get(&id)
            .map(|f| f.len())
            .unwrap_or(0)
    }

    /// Convert a Value to its WASM representation (tag, payload).
    /// Immediate values pass through directly. Heap values get a handle.
    pub fn value_to_wasm(&mut self, value: Value) -> (i64, i64) {
        let tag = value.tag;
        if tag < TAG_HEAP_START {
            // Immediate: tag and payload pass through as-is
            (tag as i64, value.payload as i64)
        } else {
            // Heap: insert into handle table, payload becomes handle
            let handle = self.handles.insert(value);
            (tag as i64, handle as i64)
        }
    }

    /// Convert WASM representation (tag, payload) back to a Value.
    /// Immediate values are reconstructed directly. Heap values are
    /// looked up in the handle table.
    pub fn wasm_to_value(&self, tag: i64, payload: i64) -> Value {
        let tag = tag as u64;
        if tag < TAG_HEAP_START {
            Value {
                tag,
                payload: payload as u64,
            }
        } else {
            self.handles.get(payload as u64)
        }
    }

    /// Dispatch a primitive call.
    ///
    /// `prim_id` indexes into the flattened primitive table.
    /// `args` are already-marshaled Values.
    /// Returns (signal_bits, result_value).
    pub fn call_primitive(&mut self, prim_id: u32, args: &[Value]) -> (SignalBits, Value) {
        let def = self.primitives[prim_id as usize];
        let heap = unsafe { &mut *self.heap_ptr() };
        // Mint the boundary's fresh result region explicitly so it can be both
        // threaded to `call_plugin` (the plugin region slot — no `NativeCtx`
        // region getter survives) and owned by the ctx the native body uses. The
        // VM comes from the host (`self.vm`), so a re-entrant primitive reaches it
        // through `ctx.vm()` (docs/impl/region/ctx.md).
        let region = heap.new_runtime_region();
        let mut ctx = crate::primitives::ctx::NativeCtx::with_region_vm(region, heap, self.vm);
        if std::ptr::fn_addr_eq(def.func, crate::plugin_api::PLUGIN_SENTINEL) {
            crate::plugin_api::call_plugin(def, &mut ctx, args, region)
        } else {
            (def.func)(&mut ctx, args)
        }
    }

    /// Handle SIG_IO from a primitive call.
    ///
    /// When inside a fiber (fiber_id_stack is non-empty), propagate
    /// SIG_IO so the scheduler can drive I/O through the event loop.
    /// Otherwise, execute I/O inline via the bound backend or SyncBackend.
    pub fn maybe_execute_io(&mut self, bits: SignalBits, value: Value) -> (SignalBits, Value) {
        if bits.raw() & SIG_IO.raw() == 0 {
            return (bits, value);
        }

        // Inside a fiber: propagate SIG_IO to the scheduler
        if !self.fiber_id_stack.is_empty() {
            return (bits, value);
        }

        // Top-level: execute I/O inline
        let request = match value.as_external::<IoRequest>() {
            Some(r) => r,
            None => return (bits, value),
        };

        if let Some(backend_val) = self.find_io_backend() {
            if let Some(async_be) = backend_val.as_external::<AnyBackend>() {
                if let Ok(_id) = async_be.0.submit(request, self.heap_ptr()) {
                    if let Ok(completions) = async_be.0.wait(-1) {
                        if let Some(c) = completions.into_iter().next() {
                            return match c.result {
                                Ok(v) => (crate::value::fiber::SIG_OK, v),
                                Err(e) => (crate::value::fiber::SIG_ERROR, e),
                            };
                        }
                    }
                }
            }
        }
        // Fallback: use the lazily-initialized backend
        self.execute_io_inline(request)
    }

    /// Execute an I/O request using the lazily-initialized backend.
    pub(crate) fn execute_io_inline(
        &mut self,
        request: &IoRequest,
    ) -> (crate::value::fiber::SignalBits, Value) {
        let backend = match &self.io_backend {
            Some(_) => self.io_backend.as_ref().unwrap(),
            None => match crate::io::aio::AsyncBackend::new_with_unicode(
                unsafe { &*self.vm }.unicode_generation(),
            ) {
                Ok(be) => {
                    self.io_backend = Some(AnyBackend(Box::new(be)));
                    self.io_backend.as_ref().unwrap()
                }
                Err(e) => {
                    let heap = unsafe { &mut *self.heap_ptr() };
                    let ctx = crate::primitives::ctx::Alloc::new(heap);
                    return (
                        crate::value::fiber::SIG_ERROR,
                        ctx.error("io-error", format!("failed to create I/O backend: {}", e)),
                    );
                }
            },
        };
        if let Ok(_id) = backend.0.submit(request, self.heap_ptr()) {
            if let Ok(completions) = backend.0.wait(-1) {
                if let Some(c) = completions.into_iter().next() {
                    return match c.result {
                        Ok(v) => (crate::value::fiber::SIG_OK, v),
                        Err(e) => (crate::value::fiber::SIG_ERROR, e),
                    };
                }
            }
        }
        let heap = unsafe { &mut *self.heap_ptr() };
        let ctx = crate::primitives::ctx::Alloc::new(heap);
        (
            crate::value::fiber::SIG_ERROR,
            ctx.error("io-error", "I/O submission failed"),
        )
    }

    /// Search param_frames for a value that is an I/O backend.
    fn find_io_backend(&self) -> Option<Value> {
        for frame in self.param_frames.iter().rev() {
            for &(_, value) in frame {
                if value.as_external::<AnyBackend>().is_some() {
                    return Some(value);
                }
            }
        }
        None
    }

    /// Resolve a parameter's current value by walking param_frames.
    pub fn resolve_parameter(&self, id: u32, default: Value) -> Value {
        for frame in self.param_frames.iter().rev() {
            for &(param_id, value) in frame {
                if param_id == id {
                    return value;
                }
            }
        }
        default
    }
}

/// Trait for host types that can prepare WASM closure environments.
/// Implemented by both ElleHost (full-module mode) and TieredHost (per-closure mode).
pub trait WasmEnvHost {
    fn env_stack_ptr(&self) -> usize;
    fn set_env_stack_ptr(&mut self, ptr: usize);
    fn value_to_wasm(&mut self, value: Value) -> (i64, i64);
    /// Raw pointer to the driving instance's heap, so the generic env builder
    /// allocates capture cells on the instance's own heap (`vm.heap_ptr`).
    fn heap_ptr(&self) -> *mut crate::value::fiberheap::FiberHeap;
}

impl WasmEnvHost for ElleHost {
    fn env_stack_ptr(&self) -> usize {
        self.env_stack_ptr
    }
    fn set_env_stack_ptr(&mut self, ptr: usize) {
        self.env_stack_ptr = ptr;
    }
    fn value_to_wasm(&mut self, value: Value) -> (i64, i64) {
        self.value_to_wasm(value)
    }
    fn heap_ptr(&self) -> *mut crate::value::fiberheap::FiberHeap {
        ElleHost::heap_ptr(self)
    }
}

/// The WASM host's dispatch table (index = `prim_id`). This is the canonical
/// registry snapshot, so the host resolves a `prim_id` to the SAME def an
/// immediate native-fn `Value{TAG_NATIVE_FN, prim_id}` names — one prim_id space
/// across the value representation, the WASM emitter, and host dispatch.
fn build_primitive_table() -> Vec<&'static PrimitiveDef> {
    crate::primitives::prim_table_snapshot()
}

/// Build a name → `prim_id` lookup for the WASM emitter, over the canonical
/// registry table — so the ids the emitter bakes agree with the host's dispatch
/// table and the immediate native-fn payloads.
pub fn build_primitive_id_map() -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    for (id, def) in crate::primitives::prim_table_snapshot().iter().enumerate() {
        let id = id as u32;
        map.insert(def.name.to_string(), id);
        for alias in def.aliases {
            map.insert((*alias).to_string(), id);
        }
    }
    map
}
