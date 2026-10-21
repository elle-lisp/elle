// audited: 2026-10-21
//! What a host, a REPL line and the standard library register into a compile context.
//!
//! docs/pipeline.md
//! docs/embedding.md
//! docs/impl/region/rules.md

use super::CompileCtx;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::value::arena::RootRef;
use std::collections::HashMap;

impl CompileCtx {
    /// Register a binding an embedder makes, so that every compile in this
    /// instance resolves it, the files a program imports included. The value
    /// is rooted as [`register_repl_binding`](Self::register_repl_binding)
    /// roots one.
    ///
    /// It empties the store, because a kept unit resolved the name as it was
    /// (docs/pipeline.md).
    pub fn register_host_binding(
        &mut self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        sym_id: crate::value::SymbolId,
        value: crate::value::Value,
        funding: RootRef,
        signal: Signal,
        arity: Option<crate::value::types::Arity>,
    ) {
        self.meta.signals.insert(sym_id, signal);
        self.meta.functions.insert(sym_id, value);
        if let Some(a) = arity {
            self.meta.arities.insert(sym_id, a);
        }
        crate::value::arena::register_process_root(heap, value, funding);
        self.store.clear();
    }

    /// Register a REPL `def` binding so later REPL lines resolve it. No other
    /// compile does: a file a line imports compiles as it would from any
    /// program.
    ///
    /// A REPL `def` value outlives the line that produced it, because later
    /// lines resolve it. So the value's region becomes a process root, which
    /// keeps it live for the session and releases it by RC at teardown.
    ///
    /// `funding` says which reference that root is made of. A caller holding
    /// the value's own reference hands it over with `RootRef::Take`. A caller
    /// registering a value it reaches through another, such as a leaf of a
    /// destructuring `def`, asks for `RootRef::Mint`. A minted root funds
    /// itself per registration, so two leaves that share one region mint two
    /// references against the sweep's two decrefs. A taken root does not: the
    /// caller holds one reference, so it registers the region once.
    pub fn register_repl_binding(
        &mut self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        sym_id: crate::value::SymbolId,
        value: crate::value::Value,
        funding: RootRef,
        signal: Signal,
        arity: Option<crate::value::types::Arity>,
    ) {
        self.repl.bind(sym_id, value, signal, arity);
        crate::value::arena::register_process_root(heap, value, funding);
    }

    /// Register a host primitive under its name and aliases, so compiled code
    /// and `eval` both resolve it as they resolve a built-in.
    ///
    /// A host binding reaches the compile `meta` alone, and `eval` compiles
    /// against the macro-body `eval_meta`; a primitive belongs in both. The
    /// macro VM's doc table learns it too, so a macro body reads it as run
    /// time does (docs/macros.md). A primitive value is an immediate, so
    /// nothing here needs a root.
    ///
    /// It empties the store, as registering a binding does.
    pub fn register_primitive(
        &mut self,
        symbols: &mut SymbolTable,
        def: &'static crate::primitives::def::PrimitiveDef,
    ) {
        let mut eval_meta = self.expander.eval_meta().clone();
        for name in def.names() {
            let sym_id = symbols.intern(name);
            self.meta.insert_def(sym_id, def);
            eval_meta.insert_def(sym_id, def);
        }
        self.expander.set_eval_meta(eval_meta);
        crate::primitives::def::Doc::enter(&mut self.vm.docs, def);
        self.store.clear();
    }

    /// Keep the macros a REPL line defined, from the expander that compiled
    /// it, so later REPL lines expand them. The instance's own macros in that
    /// expander are not the line's, and stay where they are.
    pub fn register_repl_macros(&mut self, macros: &HashMap<String, crate::syntax::MacroDef>) {
        self.repl.keep_macros(self.expander.macros(), macros);
    }

    /// Add stdlib exports to the compile `meta` (so user code sees them as
    /// globals) and to the macro-body `eval_meta` (so transformer bodies can
    /// call stdlib). Called by `init_stdlib` after executing stdlib.lisp.
    /// It empties the store, as registering a binding does.
    pub fn register_stdlib_exports(
        &mut self,
        exports: &HashMap<crate::value::SymbolId, (crate::value::Value, Signal)>,
    ) {
        for (sym_id, (value, signal)) in exports {
            self.meta.signals.insert(*sym_id, *signal);
            self.meta.functions.insert(*sym_id, *value);
        }
        // Mirror into the macro-body metadata (primitives + stdlib).
        let mut eval_meta = self.expander.eval_meta().clone();
        for (sym_id, (value, signal)) in exports {
            eval_meta.signals.insert(*sym_id, *signal);
            eval_meta.functions.insert(*sym_id, *value);
        }
        self.expander.set_eval_meta(eval_meta);
        self.store.clear();
    }
}
