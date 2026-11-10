// audited: 2026-11-10
//! The drop a reassigned captured binding's binder owes its init, in either cell realization.
//!
//! docs/impl/region/cells.md

use super::*;

/// How a binder released its init's producer reference, read off the
/// instructions that follow the init's store into the cell.
#[derive(Debug, PartialEq)]
enum InitDrop {
    /// A slot-resolved `DecrefRegion`.
    Slot,
    /// A `DecrefValueRegion` of the init value.
    Value,
}

/// A capture cell a store names: an env index, or the stack slot a compiled
/// cell was loaded from.
#[derive(Clone, Copy, PartialEq)]
enum Cell {
    Env(u16),
    Local(u16),
}

/// The drop that follows a reassigned captured binding's init store, in the
/// first function of `module` that stores into one cell twice: the init, then
/// the `assign`. `None` when the instruction after the init store drops nothing.
/// A `nil` store is neither: the `letrec` pre-pass and a compiled cell's mint
/// store `nil` before the init.
fn init_drop(module: &FrozenModule) -> Option<InitDrop> {
    for func in functions(module) {
        let instrs = flat_instrs(func);
        let mut nil_regs: Vec<Reg> = Vec::new();
        let mut loaded: Vec<(Reg, u16)> = Vec::new();
        let mut stores: Vec<(Cell, usize)> = Vec::new();
        for (at, i) in instrs.iter().enumerate() {
            match *i {
                InstrRef::Const {
                    dst,
                    value: ConstRef::Nil,
                } => nil_regs.push(dst),
                InstrRef::LoadLocal { dst, slot } => loaded.push((dst, slot)),
                InstrRef::StoreCapture { index, src } if !nil_regs.contains(&src) => {
                    stores.push((Cell::Env(index), at))
                }
                InstrRef::StoreCaptureCell { cell, value } if !nil_regs.contains(&value) => {
                    if let Some(&(_, slot)) = loaded.iter().rev().find(|(r, _)| *r == cell) {
                        stores.push((Cell::Local(slot), at));
                    }
                }
                _ => {}
            }
        }
        let Some(&(cell, at)) = stores
            .iter()
            .find(|(c, _)| stores.iter().filter(|(d, _)| d == c).count() >= 2)
        else {
            continue;
        };
        let rest: Vec<&InstrRef<'_>> = instrs[at + 1..]
            .iter()
            .filter(|i| {
                !matches!(
                    i,
                    InstrRef::AdoptCellRegion { .. } | InstrRef::AssertRegionMatches { .. }
                )
            })
            .collect();
        return match (rest.first(), rest.get(1), cell) {
            (Some(InstrRef::DecrefRegion { .. }), _, _) => Some(InitDrop::Slot),
            (Some(InstrRef::DecrefValueRegion { .. }), _, Cell::Local(_)) => Some(InitDrop::Value),
            (
                Some(InstrRef::LoadCaptureRaw { dst, index }),
                Some(InstrRef::DecrefValueRegion { src }),
                Cell::Env(stored),
            ) if *index == stored && src == dst => Some(InitDrop::Value),
            _ => None,
        };
    }
    None
}

#[test]
fn env_cell_call_init_is_dropped_by_value_under_every_binder() {
    // Inside a function, `v` takes an env cell, a call result as its init, and an
    // `assign`. Its binder skips the slot routing, so the call result has no
    // release unless the binder drops it after the store, by value, because a
    // call result's region is a runtime fact. The counter-factual: the binder
    // stored the init and dropped nothing, and the init leaked once per call
    // under each of the three binders (docs/impl/region/cells.md).
    for (binder, source) in [
        (
            "let",
            "(fn () (let [@v (f 1) g (fn () v)] (begin (assign v 2) (g))))",
        ),
        (
            "def",
            "(fn () (begin (def @v (f 1)) (def g (fn () v)) (assign v 2) (g)))",
        ),
        (
            "letrec",
            "(fn () (letrec [@v (f 1) g (fn () v)] (begin (assign v 2) (g))))",
        ),
    ] {
        assert_eq!(
            init_drop(&compile_to_lir(source)),
            Some(InitDrop::Value),
            "the {binder} binder must drop its call-result init after the env-cell store",
        );
    }
}

#[test]
fn env_cell_fresh_init_is_dropped_by_slot() {
    // A fresh allocation's region names a slot, so the binder's drop is the
    // slot-resolved `DecrefRegion`, and the region's own release by id then finds
    // the slot taken. The drop sits at the store, ahead of the tail call that ends
    // the frame. The counter-factual: only the release by id ran, and it landed
    // after that tail call, where the frame never returns to it.
    let module =
        compile_to_lir("(fn () (let [@v (%pair 1 2) g (fn () v)] (begin (assign v 2) (g))))");
    assert_eq!(
        init_drop(&module),
        Some(InitDrop::Slot),
        "a fresh init's drop must be slot-resolved, right after the env-cell store",
    );
}

#[test]
fn an_init_that_names_a_binding_is_not_dropped() {
    // `@v`'s init names `x` and allocates nothing, so it holds no producer
    // reference of its own: `x`'s binder holds the one there is. A drop at `v`'s
    // binder takes `x`'s reference, and the first `assign` then frees `x` under
    // its later read. The counter-factual: the compiled cell's binder dropped
    // every reassigned init, and the top-level shape faulted under guardfree
    // (tests/impl/region-capture-cell-init-drop.lisp).
    for (cell, source) in [
        (
            "compiled",
            "(begin (def x (%pair 1 2)) (def @v x) (def u (fn () v)) (assign v 3) (g (u) x))",
        ),
        (
            "env",
            "(fn (y) (let [@v y g (fn () v)] (begin (assign v 2) (g) y)))",
        ),
    ] {
        assert_eq!(
            init_drop(&compile_to_lir(source)),
            None,
            "the {cell} cell's binder must not drop an init that names another binding",
        );
    }
}
