// audited: 2026-11-17
//! Every `Assign` functionalize keeps marks its target mutated and not immutable in the arena.
//!
//! docs/impl/region/reads.md

use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};
use crate::symbol::SymbolTable;

/// What a functionalized tree binds and writes: every `Assign` target, every
/// `Loop` parameter, and every `Let` binding.
#[derive(Default)]
struct Bound {
    targets: Vec<Binding>,
    params: Vec<Binding>,
    lets: Vec<Binding>,
}

impl Bound {
    fn collect(&mut self, hir: &Hir) {
        match &hir.kind {
            HirKind::Assign { target, .. } => self.targets.push(*target),
            HirKind::Loop { bindings, .. } => self.params.extend(bindings.iter().map(|(b, _)| *b)),
            HirKind::Let { bindings, .. } => self.lets.extend(bindings.iter().map(|(b, _)| *b)),
            _ => {}
        }
        hir.for_each_child(|c| self.collect(c));
    }
}

/// Functionalize `source` with the graft off, so a call stays a call. Assert
/// every `Assign` target reads mutated and not immutable, and answer what the
/// tree binds and writes.
fn assert_targets_mutated(source: &str) -> Bound {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    cctx.set_graft(crate::config::GraftPolicy::Off);
    let (hir, arena): (Hir, BindingArena) =
        crate::pipeline::compile_file_to_fhir(source, &mut symbols, &mut cctx, "<test>")
            .expect("compile");
    let mut bound = Bound::default();
    bound.collect(&hir);
    assert!(
        !bound.targets.is_empty(),
        "precondition: the shape keeps an Assign"
    );
    for b in &bound.targets {
        let info = arena.get(*b);
        assert!(
            info.is_mutated && !info.is_immutable,
            "Assign target {b:?} reads is_mutated={} is_immutable={}; region \
             inference reads these flags to decide whether a read of the target \
             takes a count",
            info.is_mutated,
            info.is_immutable,
        );
    }
    bound
}

/// A `while` body's `assign` targets the loop parameter, a fresh version that
/// `fresh_version` mints immutable. The counter-factual: with the parameter
/// left immutable, `(def t c)` took no count, and the next pass's assign freed
/// the value under `t` (tests/impl/region-container-reader-uaf.lisp).
#[test]
fn every_assign_target_reads_mutated() {
    let bound = assert_targets_mutated(
        "(defn f [n]\n\
           (var c (array 0))\n\
           (var i 0)\n\
           (while (%lt i n)\n\
             (def t c)\n\
             (assign c (array i))\n\
             (length t)\n\
             (assign i (%add i 1)))\n\
           c)\n\
         (f 4)",
    );
    assert!(
        bound.targets.iter().any(|b| bound.params.contains(b)),
        "precondition: an Assign targets a loop parameter"
    );
}

/// A branch's preserved assign can target an SSA version: the `let` init forks
/// a version of `x`, and the arm's assign writes that version's slot. The
/// version is as much a 1-slot container as a loop parameter is.
#[test]
fn a_preserved_assign_marks_its_ssa_version_mutated() {
    let bound = assert_targets_mutated(
        "(defn f []\n\
           (var x 1)\n\
           (let [y (begin (assign x 3) 5)]\n\
             (if true (begin (assign x (%add x y))) nil))\n\
           x)\n\
         (f)",
    );
    assert!(
        bound.targets.iter().any(|b| bound.lets.contains(b)),
        "precondition: the arm's assign targets the version the let init forks"
    );
}
