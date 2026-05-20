//! Intrinsic operation mapping for operator specialization.
//!
//! Maps known primitive operator SymbolIds to specialized LIR instructions
//! (BinOp, CmpOp, UnaryOp) so the lowerer can emit them directly instead
//! of generic LoadGlobal + Call sequences.

use super::types::ConvOp;
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
    pub fn new(symbols: &SymbolTable, meta: &crate::primitives::def::PrimitiveMeta) -> Self {
        let intrinsics = build_intrinsics(symbols);
        let call_classification = crate::hir::CallClassification {
            intrinsic_ops: intrinsics.keys().copied().collect(),
            immediate_primitives: meta.immediate_primitives.clone(),
            ..Default::default()
        };
        PrimitiveClassification {
            intrinsics,
            call_classification,
        }
    }
}
