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
            "%add",
            "%set-add",
        ]
        .iter()
        .filter_map(|name| symbols.get(name))
        .collect();

        let call_classification = crate::hir::CallClassification {
            intrinsic_ops: intrinsics.keys().copied().collect(),
            effects: meta.effects.iter().map(|(k, v)| (*k, *v)).collect(),
            ret_types: meta.ret_types.iter().map(|(k, v)| (*k, *v)).collect(),
            embeds: meta.embeds.iter().map(|(k, v)| (*k, *v)).collect(),
            retaining_store_funnels,
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
