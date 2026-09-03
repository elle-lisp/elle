//! Compiling a source fragment to HIR, for the unit tests of the passes that
//! consume it.
//!
//! Every HIR pass — tail-call marking, def-use, ANF, liveness, region
//! inference, LIR lowering — tests against a tree built by the same front-end
//! run. Two things vary between them, and both are choices the pass makes
//! rather than accidents: how far down the pipeline to stop, and what free
//! names the fragment may mention.
//!
//! Adding a pipeline stage means changing [`HirFixture::build_into`] and
//! nothing else.

use crate::hir::arena::BindingArena;
use crate::hir::expr::Hir;
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::Analyzer;
use crate::primitives::{register_primitives, PrimitiveMeta};
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::value::Value;
use crate::vm::VM;

/// How far to run before handing the tree back.
///
/// The variants are ordered: each runs everything the one before it runs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Stage {
    /// Binding resolution only — the tree the analyzer produces.
    Analyzed,
    /// Plus tail-call marking.
    TailMarked,
    /// Plus functionalization.
    Functional,
    /// Plus ANF lifting. What every pass downstream of ANF wants.
    Anf,
}

/// The free names a fragment may mention, bound as a surrounding `letrec`.
///
/// A fragment under test is an expression, not a program, so names like `f`
/// and `g` have to come from somewhere. They are bound to stubs rather than to
/// primitives so the fragment's shape stays under the test's control.
///
/// The stub bodies matter to some passes and not to others: `(fn (& args)
/// args)` returns its argument list and so allocates, while `(fn (& args)
/// nil)` does not. A region or liveness test picks the one whose allocation
/// behavior it means to observe.
pub(crate) const STUBS_RETURNING_NIL: &str =
    "cond_var (fn () nil) f (fn (& args) nil) g (fn (& args) nil)";

/// The allocating stubs — see [`STUBS_RETURNING_NIL`].
pub(crate) const STUBS_RETURNING_ARGS: &str =
    "cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)";

/// Builds HIR from a source fragment.
pub(crate) struct HirFixture {
    stubs: &'static str,
    stage: Stage,
}

impl HirFixture {
    /// A fixture running the whole pipeline, with the non-allocating stubs.
    pub(crate) fn new() -> Self {
        HirFixture {
            stubs: STUBS_RETURNING_NIL,
            stage: Stage::Anf,
        }
    }

    /// Stop after `stage` instead of after ANF.
    pub(crate) fn stage(mut self, stage: Stage) -> Self {
        self.stage = stage;
        self
    }

    /// Bind these `letrec` bindings around the fragment instead of the default
    /// stubs. The text is a run of `name value` pairs, as `letrec` takes them.
    pub(crate) fn stubs(mut self, stubs: &'static str) -> Self {
        self.stubs = stubs;
        self
    }

    /// Compile the fragment with no surrounding `letrec` at all.
    ///
    /// For a fragment that mentions no free names and whose tree a test walks
    /// from the root: the wrapper would otherwise put a `Letrec` node there.
    pub(crate) fn bare(mut self) -> Self {
        self.stubs = "";
        self
    }

    /// Compile `source`, owning the arena and symbol table.
    pub(crate) fn build(&self, source: &str) -> (Hir, BindingArena, SymbolTable) {
        let mut symbols = SymbolTable::new();
        let mut arena = BindingArena::new();
        let built = self.build_into(source, &mut arena, &mut symbols);
        (built.hir, arena, symbols)
    }

    /// Compile `source` into a caller-owned arena and symbol table, for the
    /// tests that must outlive the fixture's own borrow of them, or that need
    /// the analyzer's by-products.
    pub(crate) fn build_into(
        &self,
        source: &str,
        arena: &mut BindingArena,
        symbols: &mut SymbolTable,
    ) -> Built {
        let mut vm = VM::new();
        let meta = register_primitives(&mut vm, symbols);

        let wrapped = if self.stubs.is_empty() {
            source.to_string()
        } else {
            format!("(letrec [{}] {})", self.stubs, source)
        };
        let syntax_arena = crate::syntax::SyntaxArena::mint(vm.heap());
        let syntax = read_syntax(syntax_arena, &wrapped, "<test>").expect("parse failed");
        let mut expander = Expander::on_vm(&mut vm);
        expander.set_arena(syntax_arena);
        let expanded = expander
            .expand(syntax, symbols, &mut vm)
            .expect("expand failed");

        let mut analyzer = Analyzer::new(symbols, arena);
        analyzer.bind_primitives(&meta);
        let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
        let primitive_values = analyzer.primitive_values().clone();
        drop(analyzer);

        if self.stage >= Stage::TailMarked {
            mark_tail_calls(&mut analysis.hir);
        }
        if self.stage >= Stage::Functional {
            functionalize(&mut analysis.hir, arena);
        }
        if self.stage >= Stage::Anf {
            crate::hir::anf::anf_lift(&mut analysis.hir, arena);
        }
        Built {
            hir: analysis.hir,
            primitive_values,
            meta,
        }
    }
}

/// What a compile produces beyond the tree: the by-products a downstream pass
/// needs to be constructed at all.
pub(crate) struct Built {
    pub(crate) hir: Hir,
    /// The analyzer's binding-to-value map for the primitives, which the LIR
    /// lowerer carries so a call to a primitive resolves without a symbol
    /// lookup.
    pub(crate) primitive_values: std::collections::HashMap<crate::hir::binding::Binding, Value>,
    /// The registration metadata, for a caller building its own classification
    /// of the primitives.
    pub(crate) meta: PrimitiveMeta,
}

#[cfg(test)]
mod tests {
    use super::{HirFixture, Stage, STUBS_RETURNING_ARGS};
    use crate::hir::expr::{Hir, HirKind};

    fn has_tail_call(hir: &Hir) -> bool {
        let mut found = false;
        fn walk(hir: &Hir, found: &mut bool) {
            if let HirKind::Call { is_tail: true, .. } = &hir.kind {
                *found = true;
            }
            hir.for_each_child(|c| walk(c, found));
        }
        walk(hir, &mut found);
        found
    }

    #[test]
    fn stopping_before_tail_marking_leaves_calls_unmarked() {
        // The counterfactual for `Stage`: were the stages not gated, a fixture
        // asking for `Analyzed` would still hand back a tail-marked tree, and
        // a tail-call test could not tell the marking pass from the fixture.
        let (early, _, _) = HirFixture::new()
            .stage(Stage::Analyzed)
            .build("(fn () (f 1))");
        assert!(
            !has_tail_call(&early),
            "no pass has marked tail calls at this stage"
        );

        let (marked, _, _) = HirFixture::new()
            .stage(Stage::TailMarked)
            .build("(fn () (f 1))");
        assert!(has_tail_call(&marked), "mark_tail_calls should have run");
    }

    #[test]
    fn the_stub_preamble_is_selectable() {
        // Both stub sets bind the same names, so a fragment mentioning them
        // compiles either way; the bodies are what differ.
        for stubs in [super::STUBS_RETURNING_NIL, STUBS_RETURNING_ARGS] {
            let (hir, _, _) = HirFixture::new().stubs(stubs).build("(f 1 2)");
            assert!(matches!(hir.kind, HirKind::Letrec { .. }));
        }
    }

    #[test]
    fn a_caller_owned_arena_receives_the_bindings() {
        use crate::hir::arena::BindingArena;
        use crate::symbol::SymbolTable;

        let mut arena = BindingArena::new();
        let mut symbols = SymbolTable::new();
        let before = arena.len();
        let built = HirFixture::new().build_into("(let [x 1] x)", &mut arena, &mut symbols);
        assert!(
            !built.primitive_values.is_empty(),
            "the primitive bindings come back with the tree"
        );
        assert!(
            arena.len() > before,
            "the fragment's bindings must land in the caller's arena"
        );
    }
}
