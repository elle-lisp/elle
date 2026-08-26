//! SendValue wrapper for thread-safe value transmission
//!
//! This module provides SendValue, a wrapper around Value that implements Send
//! by deep-copying heap values instead of sharing raw pointers.
//!
//! The problem with raw Value copies: Value contains raw pointers to Rc
//! heap objects. When sent to another thread, the original Rc may drop and free the
//! heap object while the thread still holds a raw pointer to it.
//!
//! The solution: SendValue stores owned copies of heap data, not raw pointers.

use super::heap::{deref, HeapObject, HeapTag};
use super::repr::Value;
use crate::error::LocationMap;
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::types::Arity;
use std::collections::{BTreeMap, HashMap};

mod de;
mod mirror;
mod ser;
mod syntax;

use de::{into_value_inner, template_from_sendable, DeserContext, ReconState};
use ser::{from_value_inner, sendable_from_template, SerContext};
use syntax::{send_to_syntax, SendSyntax};

/// Sendable snapshot of a closure.
///
/// All `Rc`-wrapped fields from `ClosureTemplate` are owned here.
/// Fields that are not portable across thread boundaries (`jit_code`,
/// `lir_function`, `syntax`) are absent — they are set to `None` on
/// reconstruction.
///
/// `env` holds the captured environment (upvalues), converted recursively
/// to `SendValue`. Constants are stored separately in `constants`.
///
/// This struct is `pub(crate)` — it is part of the public interface of
/// `SendBundle` but not independently useful outside `send.rs`.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SendableClosure {
    pub bytecode: Vec<u8>,
    pub arity: Arity,
    pub num_locals: usize,
    pub num_captures: usize,
    pub num_params: usize,
    pub constants: Vec<SendValue>,
    pub signal: Signal,
    pub capture_params_mask: u64,
    pub capture_locals_mask: crate::value::CaptureMask,
    pub symbol_names: HashMap<u32, String>,
    pub location_map: LocationMap,
    pub doc: Option<String>,
    pub vararg_kind: VarargKind,
    pub name: Option<String>,
    pub squelch_mask: SignalBits,
    pub env: Vec<SendValue>,
    /// LIR function for JIT compilation in spawned threads.
    /// Stripped of doc/syntax (not sendable), but retains all JIT-relevant fields.
    pub lir_function: Option<crate::lir::LirFunction>,
    /// Sendable snapshots of compound `ValueConst` operands lifted out of the
    /// LIR (quoted lists, structs, …). `convert_lir_for_send` replaces each such
    /// instruction with `LirConst::ValueRef(idx)` indexing this pool; on
    /// reconstruction `patch_lir_value_refs` rebuilds them into worker-heap
    /// `ValueConst`s. Keeping the LIR shippable lets JIT/MLIR/WASM tiers run a
    /// closure across a thread boundary instead of dropping its LIR.
    pub lir_value_pool: Vec<SendValue>,
    /// Nested-lambda blueprints (`ClosureTemplate.child_protos`) this code
    /// object's `MakeClosure` instructions index. Serialized inline (templates
    /// have no heap identity to intern) so the worker can rebuild them into the
    /// reconstructed template's `child_protos` and resolve `MakeClosure`. Each
    /// has empty `env`/`squelch_mask` — a blueprint is a pure template.
    pub child_protos: Vec<SendableClosure>,
    /// The static region slots this code object's allocations SHARE after a
    /// builder-idiom merge (`ClosureTemplate.merged_slots`; docs/impl/region/merging.md
    /// § Merging), serialized so the worker mint-or-reuses them and its region count
    /// matches the sender's. Empty unless a merge fired.
    pub merged_slots: Vec<u32>,
    /// Value-routed release slots (`ClosureTemplate.frame_release_slots`), so an
    /// abandoned frame on the worker runs the releases it still owes.
    pub frame_release_slots: Vec<u16>,
    /// Slot-routed release regions (`ClosureTemplate.frame_release_regions`).
    pub frame_release_regions: Vec<u32>,
}

/// A thread-safe wrapper around Value that deep-copies heap data.
///
/// For immediate values (nil, bool, int, float, symbol), SendValue stores
/// them directly. Keywords carry their name for cross-thread re-interning.
/// For heap values, SendValue stores owned copies of the heap data, ensuring
/// the data remains valid even if the original Rc is dropped.
#[derive(Clone)]
pub enum SendValue {
    /// Immediate values that don't need copying
    Immediate(Value),

    /// Keyword with name for cross-thread re-interning
    Keyword(String),

    /// Symbol with name for cross-thread re-interning. Symbol IDs are per-table
    /// and are NOT comparable across tables, so a symbol that appears as runtime
    /// *data* (a quoted datum, a channel message) carries its name and is
    /// re-interned in the receiving thread's table. `id` is the sender-table id,
    /// kept only as a fallback for the (unexpected) case where the receiving
    /// thread has no symbol table — then it falls back to the raw id rather than
    /// losing the value.
    Symbol { name: String, id: u32 },

    /// Owned string copy
    String(String),

    /// Deep copy of pair cells (with traits)
    Pair(Box<SendValue>, Box<SendValue>, Box<SendValue>),

    /// Deep copy of arrays (with traits)
    Array(Vec<SendValue>, Box<SendValue>),

    /// Deep copy of structs (immutable maps, with traits)
    Struct(
        BTreeMap<crate::value::heap::TableKey, SendValue>,
        Box<SendValue>,
    ),

    /// Deep copy of @structs (mutable maps, with traits)
    StructMut(
        BTreeMap<crate::value::heap::TableKey, SendValue>,
        Box<SendValue>,
    ),

    /// Deep copy of arrays (immutable fixed-length sequences, with traits)
    Tuple(Vec<SendValue>, Box<SendValue>),

    /// Deep copy of @strings (mutable byte sequences, with traits)
    Buffer(Vec<u8>, Box<SendValue>),

    /// Deep copy of @bytes (immutable binary data, with traits)
    Bytes(Vec<u8>, Box<SendValue>),

    /// Deep copy of @bytes (mutable binary data, with traits)
    Blob(Vec<u8>, Box<SendValue>),

    /// Deep copy of user boxes (if contents are sendable)
    LBox(Box<SendValue>, Box<SendValue>),

    /// Deep copy of compiler capture cells (if contents are sendable)
    CaptureCell(Box<SendValue>, Box<SendValue>),

    /// Float values that couldn't be stored inline
    Float(f64),

    /// Deep copy of FFI type descriptor (pure data, no Rc)
    FFIType(crate::ffi::types::TypeDesc),

    /// Deep copy of immutable sets (with traits)
    LSet(Vec<SendValue>, Box<SendValue>),

    /// Deep copy of mutable sets (with traits)
    LSetMut(Vec<SendValue>, Box<SendValue>),

    /// A parsed syntax tree (pre-analysis). Self-contained — see `SendSyntax`.
    Syntax(Box<SendSyntax>),

    // (Native-fns are immediates now — `Value{TAG_NATIVE_FN, prim_id}` — and ride
    // the `Immediate` arm. The prim_id is stable across the boundary via
    // deterministic registration, so no dedicated SendValue variant is needed.)
    /// Deep copy of a closure (template + captured environment).
    /// Only appears as an entry in `SendBundle::closures`.
    /// The root `SendValue` tree and closure envs reference closures via `Ref(idx)`.
    Closure(Box<SendableClosure>),

    /// Back-reference into `SendBundle::closures` by index.
    /// Meaningful only within a `SendBundle`; a bare `Ref` without a bundle is invalid.
    Ref(usize),

    /// Cloned crossbeam channel sender plus the shared `WakeList` so a
    /// `chan/send` on the receiving thread can wake any `chan/select`
    /// parked on the original-thread receiver.
    #[allow(private_interfaces)]
    ChanSender(
        crossbeam_channel::Sender<crate::primitives::chan::SendableValue>,
        std::sync::Arc<crate::primitives::chan::WakeList>,
    ),

    /// Cloned crossbeam channel receiver plus the shared `WakeList`.
    #[allow(private_interfaces)]
    ChanReceiver(
        crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
        std::sync::Arc<crate::primitives::chan::WakeList>,
    ),

    /// Dynamic parameter (Racket-style). The global `id` is preserved across the
    /// boundary — parameter resolution is by id (`vm::parameters`), so even if a
    /// parameter is reachable from the graph more than once and deep-copied into
    /// distinct heap objects, every copy resolves identically. `default`/`traits`
    /// are sent recursively (so a parameter is sendable iff they are). This is
    /// what lets a closure that closes over `*stdout*`/`*stderr*` (anything using
    /// `println`) cross `os/spawn`.
    Parameter {
        id: u32,
        default: Box<SendValue>,
        traits: Box<SendValue>,
    },

    /// A standard-stream port (stdin/stdout/stderr). These do not own their fd,
    /// so they are reconstructed fresh in the receiving thread (a snapshot-send,
    /// like a fiber inheriting parameter bindings). File and socket ports stay
    /// unsendable — their fd is owned and not meaningful in another VM.
    #[allow(private_interfaces)]
    StdioPort(crate::port::PortKind),
}

/// Unit of cross-thread value transfer.
///
/// All closures reachable from `root` — including nested and mutually recursive
/// ones — are stored flat in `closures`. The root value tree and all closure
/// envs reference closures by index via `SendValue::Ref(idx)`.
///
/// The type carried by `ThreadHandle::result`.
#[derive(Clone)]
pub struct SendBundle {
    /// Root value. May contain `Ref(idx)` nodes pointing into `closures`.
    pub root: SendValue,
    /// Intern table of all closures reachable from `root`.
    pub closures: Vec<SendableClosure>,
}

// SAFETY: SendBundle owns all its data — no Rc, no RefCell.
unsafe impl Send for SendBundle {}
unsafe impl Sync for SendBundle {}

impl SendValue {
    /// Convert a Value to SendValue by deep-copying heap data.
    ///
    /// Returns Err if the value contains non-sendable data (mutable @structs,
    /// native functions, FFI handles, etc.).
    ///
    /// Note: this wrapper asserts that no closures are encountered. For values
    /// that may contain closures, use `SendBundle::from_value` instead.
    pub fn from_value(
        value: Value,
        heap: &crate::value::fiberheap::FiberHeap,
        symbols: &crate::symbol::SymbolTable,
    ) -> Result<Self, String> {
        let mut ctx = SerContext::new(heap, symbols);
        let sv = from_value_inner(value, &mut ctx)?;
        if !ctx.closures.is_empty() {
            panic!("SendValue::from_value cannot serialize closures; use SendBundle::from_value instead");
        }
        Ok(sv)
    }

    /// Convert SendValue back into a Value by reconstructing heap objects into
    /// the call's region through `ctx`, re-interning any symbol names into the
    /// receiver's `symbols` (both threaded explicitly).
    pub fn into_value(
        self,
        ctx: &mut crate::primitives::ctx::Alloc,
        symbols: &mut crate::symbol::SymbolTable,
    ) -> Value {
        // A bare `SendValue` carries no closures (`from_value` panics if any are
        // reachable), so reconstruction is the closure-free subset of
        // `into_value_inner` — delegate through an empty `DeserContext` rather
        // than duplicate every heap-object arm.
        let mut dctx = DeserContext::new(Vec::new(), ctx, symbols);
        into_value_inner(self, &mut dctx)
    }
}

// SAFETY: SendValue is safe to send because it owns all its data
unsafe impl Send for SendValue {}
unsafe impl Sync for SendValue {}

impl SendBundle {
    /// Serialize a `Value` into a `SendBundle`.
    ///
    /// Closures — including mutually recursive ones — are placed in the intern
    /// table and referenced by index via `SendValue::Ref`. The root `SendValue`
    /// may itself be a `Ref(0)` if `value` is a closure.
    ///
    /// Returns `Err` if any value in the reachable graph is not sendable
    /// (e.g., mutable @struct, fiber, FFI handle).
    pub fn from_value(
        value: Value,
        heap: &crate::value::fiberheap::FiberHeap,
        symbols: &crate::symbol::SymbolTable,
    ) -> Result<Self, String> {
        let mut ctx = SerContext::new(heap, symbols);
        let root = from_value_inner(value, &mut ctx)?;
        Ok(SendBundle {
            root,
            closures: ctx.closures,
        })
    }

    /// Reconstruct a `Value` from this bundle.
    ///
    /// Mutually recursive closures are handled via LBox fixups: if a closure's
    /// env contains an LBox wrapping a not-yet-built closure, the LBox is
    /// allocated with a NIL placeholder and updated after all closures are built.
    pub fn into_value(
        self,
        ctx: &mut crate::primitives::ctx::Alloc,
        symbols: &mut crate::symbol::SymbolTable,
    ) -> Value {
        let mut dctx = DeserContext::new(self.closures, ctx, symbols);

        let result = into_value_inner(self.root, &mut dctx);

        // Fixup pass: patch LBox cells that were given NIL placeholders.
        for (lbox_val, idx) in &dctx.fixups {
            let closure_val = match dctx.states[*idx] {
                ReconState::Done(v) => v,
                _ => panic!(
                    "bug: fixup references closure that was never built (idx={})",
                    idx
                ),
            };
            if let Some(cell) = lbox_val.as_box_or_capture_raw() {
                *cell.borrow_mut() = closure_val;
            }
        }

        result
    }
}

/// Serialize a list of closure templates (e.g. a module's `child_protos`) into
/// owned `SendableClosure`s, for the stdlib compilation cache.
///
/// Each template is a blueprint: `env`/`squelch_mask` are empty. Templates have
/// no heap identity to intern, so they are emitted inline; their own
/// `child_protos` recurse. Closure constants *inside* a template's constant
/// pool are live heap instances and intern into `ctx.closures`, which is
/// returned as the second element — the reconstructed templates reference it
/// by `Ref(idx)`.
pub(crate) fn serialize_templates(
    protos: &[std::rc::Rc<crate::value::ClosureTemplate>],
    heap: &crate::value::fiberheap::FiberHeap,
    symbols: &crate::symbol::SymbolTable,
) -> Result<(Vec<SendableClosure>, Vec<SendableClosure>), String> {
    let mut ctx = SerContext::new(heap, symbols);
    let mut out = Vec::with_capacity(protos.len());
    for t in protos {
        out.push(sendable_from_template(t, &mut ctx)?);
    }
    Ok((out, ctx.closures))
}

/// Reconstruct closure templates from owned `SendableClosure`s (the `(templates,
/// intern_table)` pair from `serialize_templates`). The intern table is shared
/// across all templates so `Ref(idx)` entries (closure constants) resolve.
///
/// `Alloc` is the receiving context's allocation capability (every
/// reconstructed heap object is born in its region); symbols re-intern names.
///
/// The stored `symbol_names` maps and LIR `LirConst::Symbol` ids carry the
/// *storing* process's ids; both are remapped to this process's table before
/// reconstruction (constants themselves re-intern by name in
/// `template_from_sendable`).
pub(crate) fn deserialize_templates(
    templates: Vec<SendableClosure>,
    mut intern_table: Vec<SendableClosure>,
    alloc: &mut crate::primitives::ctx::Alloc<'_>,
    symbols: &mut crate::symbol::SymbolTable,
) -> Result<Vec<std::rc::Rc<crate::value::ClosureTemplate>>, String> {
    remap_sendable_symbols(&mut intern_table, symbols);
    let mut templates = templates;
    remap_sendable_symbols(&mut templates, symbols);
    let mut dctx = DeserContext::new(intern_table, alloc, symbols);
    let mut out = Vec::with_capacity(templates.len());
    for sc in templates {
        out.push(template_from_sendable(sc, &mut dctx));
    }
    Ok(out)
}

/// Rewrite a `SendableClosure`'s `symbol_names` map and LIR symbol ids from the
/// storing process's table to this one. Recurses through `child_protos`.
fn remap_sendable_symbols(
    protos: &mut [SendableClosure],
    symbols: &mut crate::symbol::SymbolTable,
) {
    fn fix_one(sc: &mut SendableClosure, symbols: &mut crate::symbol::SymbolTable) {
        let old_names: Vec<(u32, String)> = sc
            .symbol_names
            .iter()
            .map(|(&id, name)| (id, name.clone()))
            .collect();
        let mut old_to_new = HashMap::new();
        let mut new_names = HashMap::new();
        for (old_id, name) in &old_names {
            let new_id = symbols.intern(name).0;
            old_to_new.insert(*old_id, new_id);
            new_names.entry(new_id).or_insert_with(|| name.clone());
        }
        if let Some(lir) = sc.lir_function.as_mut() {
            remap_lir_symbols(lir, &old_to_new);
        }
        sc.symbol_names = new_names;
        for child in sc.child_protos.iter_mut() {
            fix_one(child, symbols);
        }
    }
    for p in protos.iter_mut() {
        fix_one(p, symbols);
    }
}

/// Re-map every `LirConst::Symbol(old_id)` in a LIR function to the id that
/// `name` has in the *loading* process's symbol table. Symbol ids are
/// per-process: the JIT materializes them into `Value::symbol(id)` directly.
fn remap_lir_symbols(lir: &mut crate::lir::LirFunction, old_to_new: &HashMap<u32, u32>) {
    use crate::lir::{LirConst, LirInstr};
    for block in &mut lir.blocks {
        for si in &mut block.instructions {
            let fix = |c: &mut LirConst| {
                if let LirConst::Symbol(sid) = c {
                    if let Some(&new_id) = old_to_new.get(&sid.0) {
                        *sid = crate::value::SymbolId(new_id);
                    }
                }
            };
            match &mut si.instr {
                LirInstr::Const { value, .. } => fix(value),
                LirInstr::StructGetOrNil { key, .. } => fix(key),
                LirInstr::StructGetDestructure { key, .. } => fix(key),
                LirInstr::StructRest { exclude_keys, .. } => {
                    for k in exclude_keys.iter_mut() {
                        fix(k);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests;
