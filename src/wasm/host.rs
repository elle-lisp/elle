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

use crate::io::backend::SyncBackend;
use crate::io::request::IoRequest;
use crate::io::AnyBackend;
use crate::primitives::def::PrimitiveDef;
use crate::primitives::registration::ALL_TABLES;
use crate::signals::SIG_IO;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

use super::handle::HandleTable;

/// Base address for the env stack in linear memory.
/// Each `call_wasm_closure` allocates a region starting from here.
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
    /// Stack of suspension frames for yield/resume. Innermost (callee)
    /// frame is last. On resume, frames are popped from the end.
    pub suspension_frames: Vec<WasmSuspensionFrame>,
    /// Resume value passed by the scheduler (fiber/resume). Set before
    /// re-invoking a suspended function; consumed by rt_get_resume_value.
    pub resume_value: Option<(i64, i64)>,
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
            suspension_frames: Vec::new(),
            resume_value: None,
        }
    }
}

impl Default for ElleHost {
    fn default() -> Self {
        Self::new()
    }
}

impl ElleHost {
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
        (def.func)(args)
    }

    /// If the signal contains SIG_IO, extract the IoRequest and execute
    /// it using the backend from `*io-backend*` in the parameter system.
    /// Falls back to a fresh sync backend if no parameter is bound.
    pub fn maybe_execute_io(&self, bits: SignalBits, value: Value) -> (SignalBits, Value) {
        if bits.0 & SIG_IO.0 == 0 {
            return (bits, value);
        }
        if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
            eprintln!(
                "[wasm-io] SIG_IO intercepted, value type: {}",
                value.type_name()
            );
        }
        let request = match value.as_external::<IoRequest>() {
            Some(r) => r,
            None => {
                if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
                    eprintln!("[wasm-io] value is NOT an IoRequest, propagating signal");
                }
                return (bits, value);
            }
        };
        if std::env::var_os("ELLE_WASM_DEBUG").is_some() {
            eprintln!("[wasm-io] executing IoRequest: {:?}", request.op);
        }
        // Look up *io-backend* from param_frames. The backend is a
        // Value::external with type "io-backend" — either AnyBackend
        // (async: uring/threads) or SyncBackend.
        if let Some(backend_val) = self.find_io_backend() {
            if let Some(async_be) = backend_val.as_external::<AnyBackend>() {
                if let Ok(_id) = async_be.0.submit(request) {
                    if let Ok(completions) = async_be.0.wait(-1) {
                        if let Some(c) = completions.into_iter().next() {
                            return match c.result {
                                Ok(v) => (SIG_OK, v),
                                Err(e) => (SIG_ERROR, e),
                            };
                        }
                    }
                }
            }
            if let Some(sync_be) = backend_val.as_external::<SyncBackend>() {
                return sync_be.execute(request);
            }
        }
        // Fallback: create a temporary sync backend
        SyncBackend::new().execute(request)
    }

    /// Search param_frames for a value that is an I/O backend.
    fn find_io_backend(&self) -> Option<Value> {
        for frame in self.param_frames.iter().rev() {
            for &(_, value) in frame {
                if value.as_external::<AnyBackend>().is_some()
                    || value.as_external::<SyncBackend>().is_some()
                {
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

/// Build a flattened dispatch table from ALL_TABLES.
///
/// Each primitive gets a sequential index. This table is used by the
/// WASM emitter to assign prim_ids and by the host to dispatch calls.
fn build_primitive_table() -> Vec<&'static PrimitiveDef> {
    let mut table = Vec::new();
    for primitives in ALL_TABLES {
        for def in *primitives {
            table.push(def);
        }
    }
    table
}

/// Build a name → prim_id lookup for the WASM emitter.
///
/// Maps primitive names (and aliases) to their dispatch table index.
pub fn build_primitive_id_map() -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    let mut id: u32 = 0;
    for primitives in ALL_TABLES {
        for def in *primitives {
            map.insert(def.name.to_string(), id);
            for alias in def.aliases {
                map.insert((*alias).to_string(), id);
            }
            id += 1;
        }
    }
    map
}
