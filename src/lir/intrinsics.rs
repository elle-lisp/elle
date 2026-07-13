//! Intrinsic operation mapping for operator specialization.
//!
//! Maps known primitive operator SymbolIds to specialized LIR instructions
//! (BinOp, CmpOp, UnaryOp) so the lowerer can emit them directly instead
//! of generic LoadGlobal + Call sequences.

use super::types::ConvOp;
use crate::primitives::def::PrimitiveMeta;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use rustc_hash::FxHashMap;

/// A known intrinsic operation that can be compiled to specialized instructions.
#[derive(Debug, Clone, Copy)]
pub enum IntrinsicOp {
    Conversion(ConvOp),
}

/// Build the intrinsics map from a symbol table.
pub(crate) fn build_intrinsics(symbols: &SymbolTable) -> FxHashMap<SymbolId, IntrinsicOp> {
    let mut map = FxHashMap::default();

    let mut add = |name: &str, op: IntrinsicOp| {
        if let Some(id) = symbols.get(name) {
            map.insert(id, op);
        }
    };

    add("float", IntrinsicOp::Conversion(ConvOp::IntToFloat));
    add("integer", IntrinsicOp::Conversion(ConvOp::FloatToInt));
    add("int", IntrinsicOp::Conversion(ConvOp::FloatToInt));

    map
}

/// All primitive property sets needed by the Lowerer, built once.
pub struct PrimitiveClassification {
    pub intrinsics: FxHashMap<SymbolId, IntrinsicOp>,
    pub call_classification: crate::hir::CallClassification,
}

impl PrimitiveClassification {
    pub fn new(symbols: &SymbolTable, meta: &PrimitiveMeta) -> Self {
        let intrinsics = build_intrinsics(symbols);

        // The value-retaining store funnels — the `Funnel` ops whose runtime body
        // increfs the stored heap value (the put/push/add family). NOT the
        // removals (`%del`) or byte-copy pushes (`%string-push`/`%bytes-push`),
        // which are `Funnel` too but do not raise a stored value's RC. See
        // `CallClassification::retaining_store_funnels`.
        let retaining_store_funnels = [
            "%put",
            "%put-struct",
            "%put-struct-mut",
            "%put-array",
            "%put-array-mut",
            "%array-push",
            "%push-array",
            "%push-array-mut",
            "%add-set",
            "%add-set-mut",
        ]
        .iter()
        .filter_map(|name| symbols.get(name))
        .collect();

        // The BYTE-COPY store funnels — `Funnel` ops that COPY the pushed value's
        // bytes into the container rather than retaining its region
        // (`%string-push`/`%bytes-push`). Neither retaining (no member incref — absent
        // from `retaining_store_funnels`) NOR removing (no in-body decref — unlike
        // `%del`). A dispatch wrapper stores the value through such a funnel in ONE arm
        // but its `val` param is used across arms, so `val`'s owned reference strands on
        // the sibling arms. Because the byte-copy neither increfs nor decrefs `val`, a
        // per-arm release there is the value's ACTUAL last-use release (not a redundant
        // strand, not a double-free — the `%del` in-body decref hazard the compensation
        // excludes does NOT apply). See `CallClassification::bytecopy_store_funnels`.
        let bytecopy_store_funnels = ["%string-push", "%string-push-mut", "%bytes-push"]
            .iter()
            .filter_map(|name| symbols.get(name))
            .collect();

        // The moves-out ∩ PassThrough natives (`%pop`/`%pop-array*`): a non-fresh
        // element removed from a container, escape-retained in-body — so its tail
        // ReturnValue retain is redundant (`CallClassification::moves_out_passthrough`).
        // Derived from the def flags so a new monomorphic pop variant classifies by
        // declaring `moves_out: true` + `effect: PassThrough`, no name-list edit. A
        // moves-out native with a FRESH result (`@string`/`@bytes` pop, `Funnel`/
        // `Immediate`) is excluded — its result needs the tail retain.
        let moves_out_passthrough = meta
            .moves_out
            .iter()
            .filter(|(id, &mo)| {
                mo && meta.effects.get(id)
                    == Some(&crate::primitives::def::RegionEffect::PassThrough)
            })
            .map(|(id, _)| *id)
            .collect();

        // ALL moves-out natives, regardless of effect — the `pop` wrapper's container
        // strand (`CallClassification::moves_out`) affects the fresh-result `%pop-string`/
        // `%pop-bytes` arms too, not just the PassThrough `%pop` arm.
        let moves_out = meta
            .moves_out
            .iter()
            .filter(|(_, &mo)| mo)
            .map(|(id, _)| *id)
            .collect();

        let call_classification = crate::hir::CallClassification {
            intrinsic_ops: intrinsics.keys().copied().collect(),
            effects: meta.effects.iter().map(|(k, v)| (*k, *v)).collect(),
            ret_types: meta.ret_types.iter().map(|(k, v)| (*k, *v)).collect(),
            embeds: meta.embeds.iter().map(|(k, v)| (*k, *v)).collect(),
            retaining_store_funnels,
            bytecopy_store_funnels,
            moves_out_passthrough,
            moves_out,
            // The two natives the transferred-returned-subtree cut recognizes
            // structurally: a fiber-body producer and its resume consumer.
            fiber_new: symbols.get("fiber/new"),
            fiber_resume: symbols.get("fiber/resume"),
            ..Default::default()
        };
        PrimitiveClassification {
            intrinsics,
            call_classification,
        }
    }
}
