use super::*;
use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
use crate::hir::testkit::{HirFixture, STUBS_RETURNING_ARGS};
use crate::hir::BindingArena;
use crate::symbol::SymbolTable;

fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
    let (hir, arena, symbols) = HirFixture::new().build(source);
    let info = analyze_dataflow(&hir);
    (arena, symbols, info)
}

fn find_binding(
    info: &DataflowInfo,
    arena: &BindingArena,
    symbols: &SymbolTable,
    name: &str,
) -> Option<Binding> {
    info.def_site
        .keys()
        .find(|&&b| symbols.name(arena.get(b).name) == Some(name))
        .copied()
}

fn is_live_anywhere(info: &DataflowInfo, b: Binding) -> bool {
    info.binding_index
        .get(&b)
        .is_some_and(|&idx| info.live_out.values().any(|live| live.contains(idx)))
}

/// As `analyze`, but with the allocating stubs, and handing the tree back for
/// tests that inspect nodes rather than only the dataflow result.
fn analyze_with_hir(source: &str) -> (super::Hir, BindingArena, SymbolTable, DataflowInfo) {
    let (hir, arena, symbols) = HirFixture::new().stubs(STUBS_RETURNING_ARGS).build(source);
    let info = analyze_dataflow(&hir);
    (hir, arena, symbols, info)
}

/// Find every Call whose func is the named primitive.
fn find_calls_to_primitive(
    hir: &super::Hir,
    name: &str,
    arena: &BindingArena,
    symbols: &SymbolTable,
) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(
        hir: &super::Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
        out: &mut Vec<HirId>,
    ) {
        if let HirKind::Call { func, .. } = &hir.kind {
            if let HirKind::Var(b) = &func.kind {
                if symbols.name(arena.get(*b).name) == Some(name) {
                    out.push(hir.id);
                }
            }
        }
        hir.for_each_child(|c| walk(c, name, arena, symbols, out));
    }
    walk(hir, name, arena, symbols, &mut out);
    out
}

/// Find every Var with the given binding name.
fn find_vars_by_name(
    hir: &super::Hir,
    name: &str,
    arena: &BindingArena,
    symbols: &SymbolTable,
) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(
        hir: &super::Hir,
        name: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
        out: &mut Vec<HirId>,
    ) {
        if let HirKind::Var(b) = &hir.kind {
            if symbols.name(arena.get(*b).name) == Some(name) {
                out.push(hir.id);
            }
        }
        hir.for_each_child(|c| walk(c, name, arena, symbols, out));
    }
    walk(hir, name, arena, symbols, &mut out);
    out
}

/// Find the immediate parent of `target` in the HIR tree.
///
/// ANF lifting wraps tail expressions in a synthetic `Return(...)`
/// (and discarded statements in `Let([t = e], Var(t))`), so a value's
/// `last_use` is keyed off that wrap node rather than the `Var` leaf it
/// encloses — same region-death site, one level up. The last-use tests
/// use this to accept either the use leaf or its ANF wrap.
fn find_parent(hir: &super::Hir, target: HirId) -> Option<HirId> {
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            if c.id == target {
                found = Some(hir.id);
            } else {
                found = find_parent(c, target);
            }
        }
    });
    found
}

/// Find the first Emit node.
fn find_first_emit(hir: &super::Hir) -> Option<HirId> {
    if matches!(&hir.kind, HirKind::Emit { .. }) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = find_first_emit(c);
        }
    });
    found
}

/// Find the `Define` node that binds the named binding.
fn find_define_by_name(
    hir: &super::Hir,
    name: &str,
    arena: &BindingArena,
    symbols: &SymbolTable,
) -> Option<HirId> {
    if let HirKind::Define { binding, .. } = &hir.kind {
        if symbols.name(arena.get(*binding).name) == Some(name) {
            return Some(hir.id);
        }
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = find_define_by_name(c, name, arena, symbols);
        }
    });
    found
}

fn find_first_loop(hir: &super::Hir) -> Option<HirId> {
    if matches!(&hir.kind, HirKind::Loop { .. }) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = find_first_loop(c);
        }
    });
    found
}

/// The compiled tree alone, with no stub `letrec` around it, for the tests
/// that walk from the root.
fn hir_of(source: &str) -> crate::hir::Hir {
    HirFixture::new().bare().build(source).0
}

// `Hir` must be nameable from the test submodules below (their bodies use
// `super::Hir`); bring it into this module's scope explicitly.
use crate::hir::Hir;

mod dataflow;
mod lastuse;
mod loops;
mod order;
