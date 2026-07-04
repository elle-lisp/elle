use super::*;
use crate::hir::dataflow::{analyze_dataflow, DataflowInfo};
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

fn analyze(source: &str) -> (BindingArena, SymbolTable, DataflowInfo) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);

    let wrapped = format!(
        "(letrec [cond_var (fn () nil) f (fn (& args) nil) g (fn (& args) nil)] {})",
        source
    );
    let syntax = read_syntax(&wrapped, "<test>").expect("parse failed");
    let mut expander = Expander::new();
    let expanded = expander
        .expand(syntax, &mut symbols, &mut vm)
        .expect("expand failed");
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);

    let info = analyze_dataflow(&analysis.hir);
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

fn analyze_with_hir(source: &str) -> (super::Hir, BindingArena, SymbolTable, DataflowInfo) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);
    let wrapped = format!(
        "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
        source
    );
    let syntax = read_syntax(&wrapped, "<test>").expect("parse");
    let mut expander = Expander::new();
    let expanded = expander
        .expand(syntax, &mut symbols, &mut vm)
        .expect("expand");
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded).expect("analyze");
    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);
    let info = analyze_dataflow(&analysis.hir);
    (analysis.hir, arena, symbols, info)
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

fn hir_of(source: &str) -> crate::hir::Hir {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);
    let syntax = read_syntax(source, "<test>").expect("parse failed");
    let mut expander = Expander::new();
    let expanded = expander
        .expand(syntax, &mut symbols, &mut vm)
        .expect("expand failed");
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new(&mut symbols, &mut arena);
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded).expect("analyze failed");
    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, &mut arena);
    analysis.hir
}

// `Hir` must be nameable from the test submodules below (their bodies use
// `super::Hir`); bring it into this module's scope explicitly.
use crate::hir::Hir;

mod dataflow;
mod lastuse;
mod loops;
mod order;
