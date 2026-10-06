// audited: 2026-10-06
//! The REPL layer: the macros and definitions earlier REPL lines made, which a
//! later REPL line sees and no other compile does.
//!
//! docs/pipeline.md

use crate::primitives::def::PrimitiveMeta;
use crate::signals::Signal;
use crate::syntax::{Expander, MacroDef};
use crate::value::types::Arity;
use crate::value::{SymbolId, Value};
use std::collections::HashMap;

/// Which macros and bindings a compile expands and resolves against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// What every compile in the instance sees: the primitives, core.lisp,
    /// the prelude, the standard library and the bindings an embedder
    /// registers.
    Instance,
    /// The instance's layer with the REPL layer over it: what a REPL line
    /// sees.
    Repl,
}

/// What earlier REPL lines defined.
#[derive(Default)]
pub(super) struct ReplLayer {
    macros: HashMap<String, MacroDef>,
    /// The definitions a later line made again under the same name. Nothing
    /// expands them, and teardown releases their transformers with the rest.
    replaced: Vec<MacroDef>,
    signals: HashMap<SymbolId, Signal>,
    functions: HashMap<SymbolId, Value>,
    arities: HashMap<SymbolId, Arity>,
}

impl ReplLayer {
    /// Lay this layer over clones of the instance's expander and meta, so that
    /// a REPL definition shadows an instance one of the same name.
    pub(super) fn overlay(&self, expander: &mut Expander, meta: &mut PrimitiveMeta) {
        expander.merge_macros(&self.macros);
        for (&sym_id, &signal) in &self.signals {
            meta.signals.insert(sym_id, signal);
            meta.functions.insert(sym_id, self.functions[&sym_id]);
            match self.arities.get(&sym_id) {
                Some(&arity) => meta.arities.insert(sym_id, arity),
                None => meta.arities.remove(&sym_id),
            };
        }
    }

    /// Keep each of a line's `macros` that the instance did not define: a
    /// macro the line defined, or defined again over an instance macro.
    pub(super) fn keep_macros(
        &mut self,
        instance: &HashMap<String, MacroDef>,
        macros: &HashMap<String, MacroDef>,
    ) {
        for (name, def) in macros {
            if instance
                .get(name)
                .is_some_and(|own| own.same_definition(def))
            {
                continue;
            }
            if let Some(old) = self.macros.insert(name.clone(), def.clone()) {
                if !old.same_definition(def) {
                    self.replaced.push(old);
                }
            }
        }
    }

    /// Bind `sym_id` for later lines. A value with no arity drops the arity an
    /// earlier definition of the name left.
    pub(super) fn bind(
        &mut self,
        sym_id: SymbolId,
        value: Value,
        signal: Signal,
        arity: Option<Arity>,
    ) {
        self.signals.insert(sym_id, signal);
        self.functions.insert(sym_id, value);
        match arity {
            Some(arity) => self.arities.insert(sym_id, arity),
            None => self.arities.remove(&sym_id),
        };
    }

    /// Release every transformer a REPL macro holds. Part of the teardown
    /// sweep: this layer is the last holder of each cell it keeps.
    pub(super) fn release(&mut self, heap: &mut crate::value::fiberheap::FiberHeap) {
        for def in self.macros.values().chain(&self.replaced) {
            def.release_transformer(heap);
        }
    }
}
