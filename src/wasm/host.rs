//! Wasmtime host state and primitive dispatch.
//!
//! The host state (`ElleHost`) lives in the Wasmtime `Store` and holds:
//! - Handle table for heap objects
//! - Flattened primitive dispatch table
//! - Documentation store
//!
//! Host functions are registered as Wasmtime imports under the "elle"
//! namespace. The main one is `call_primitive(prim_id, args_ptr, nargs, ctx)`
//! which dispatches to Elle's 331+ primitive functions.

use crate::primitives::def::PrimitiveDef;
use crate::primitives::registration::ALL_TABLES;
use crate::value::fiber::SignalBits;
use crate::value::repr::TAG_HEAP_START;
use crate::value::Value;

use super::handle::HandleTable;

/// Host state stored in the Wasmtime Store<ElleHost>.
pub struct ElleHost {
    /// Handle table for heap objects.
    pub handles: HandleTable,
    /// Flattened primitive dispatch table.
    /// Index = prim_id, value = &'static PrimitiveDef.
    pub primitives: Vec<&'static PrimitiveDef>,
}

impl ElleHost {
    pub fn new() -> Self {
        let primitives = build_primitive_table();
        ElleHost {
            handles: HandleTable::new(),
            primitives,
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
