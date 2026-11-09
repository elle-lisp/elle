// audited: 2026-11-09
//! `WasmClosureMeta` and `PayloadParts::wasm_closure`: the code object of a closure a compiled WASM module builds.
//!
//! docs/impl/region/template.md
//! docs/impl/wasm.md

use crate::signals::Signal;
use crate::value::types::Arity;
use crate::value::CaptureMask;

use super::arena::PayloadParts;
use super::header::ClosureTemplate;

/// The shape of the frame a WASM closure runs in, as the `MakeClosure` site
/// supplies it. It is what a lambda's own frozen LIR would answer, read out of
/// linear memory instead, because the host has no LIR to ask.
///
/// A struct rather than eight arguments: the three counts and the two masks are
/// each a bare integer, and at a call site of eight positions a pair of them
/// swapped type-checks and runs.
pub struct WasmClosureMeta {
    pub arity: Arity,
    pub num_locals: usize,
    pub num_captures: usize,
    pub num_params: usize,
    pub signal: Signal,
    pub capture_params_mask: u64,
    pub capture_locals_mask: CaptureMask,
    /// The module function-table index of this closure's compiled body, which
    /// is how `rt_call` dispatches to it.
    pub wasm_func_idx: u32,
}

impl PayloadParts {
    /// A closure a compiled WASM module builds: the code half off `code`, the
    /// module's own payload for the closure, and the shape half off the call
    /// (docs/impl/region/template.md). `None` is a module carrying no payload,
    /// whose code object has no bytecode to run and takes its shape from `meta`
    /// alone.
    pub fn wasm_closure(code: Option<&ClosureTemplate>, meta: WasmClosureMeta) -> Self {
        let shape = |parts: PayloadParts| PayloadParts {
            num_locals: meta.num_locals,
            num_captures: meta.num_captures,
            num_params: meta.num_params,
            signal: meta.signal,
            capture_params_mask: meta.capture_params_mask,
            capture_locals_mask: meta.capture_locals_mask,
            wasm_func_idx: Some(meta.wasm_func_idx),
            ..parts
        };
        match code {
            None => shape(PayloadParts::new(Vec::new(), meta.arity, Vec::new())),
            Some(code) => shape(PayloadParts {
                location_map: code.location_map(),
                merged_slots: code.merged_slots().as_slice().to_vec(),
                frame_release_slots: code.frame_release_slots().to_vec(),
                frame_release_regions: code.frame_release_regions().to_vec(),
                children: code.payload().children().to_vec(),
                captures: code.payload().captures().to_vec(),
                ..PayloadParts::new(
                    code.bytecode().to_vec(),
                    meta.arity,
                    code.constants().to_vec(),
                )
            }),
        }
    }
}
