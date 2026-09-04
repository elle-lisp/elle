//! Shared helpers for the regions analysis tests. Free functions only —
//! every `#[test]` lives in a themed sibling submodule (see `mod.rs`).
use super::*;
use crate::value::SymbolId;

/// Compile Elle source to canonical (functionalized) HIR with a fresh per-call
/// `CompileCtx` (every compile names its instance's compile state explicitly,
/// threading the compile context as a parameter — docs/impl/region/ctx.md).
pub(super) fn compile_fhir(source: &str, symbols: &mut SymbolTable) -> (Hir, BindingArena) {
    let mut cctx = crate::pipeline::CompileCtx::new();
    crate::pipeline::compile_file_to_fhir(source, symbols, &mut cctx, "<test>").expect("compile")
}

/// The compiled tree's region inference. Uses the allocating stubs: a stub
/// returning its argument list allocates, which is what these tests observe.
pub(super) fn analyze(source: &str) -> (BindingArena, SymbolTable, RegionInfo) {
    let (hir, arena, symbols) = crate::hir::testkit::HirFixture::new()
        .stubs(crate::hir::testkit::STUBS_RETURNING_ARGS)
        .build(source);
    let info = analyze_regions(&hir, &arena);
    (arena, symbols, info)
}

/// Collect HirIds of Loop nodes in the HIR tree.
pub(super) fn find_loops(hir: &Hir) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::Loop { .. }) {
            out.push(hir.id);
        }
        hir.for_each_child(|child| walk(child, out));
    }
    walk(hir, &mut out);
    out
}

/// Collect HirIds of Let nodes in the HIR tree.
pub(super) fn find_lets(hir: &Hir) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::Let { .. }) {
            out.push(hir.id);
        }
        hir.for_each_child(|child| walk(child, out));
    }
    walk(hir, &mut out);
    out
}

pub(super) fn count_live_scopes(info: &RegionInfo) -> usize {
    info.scope_region
        .values()
        .filter(|r| info.live_regions.contains(r))
        .count()
}

pub(super) fn count_empty_scopes(info: &RegionInfo) -> usize {
    info.scope_region
        .values()
        .filter(|r| !info.live_regions.contains(r))
        .count()
}

/// Compile Elle source through the real pipeline and return the HIR,
/// arena, and RegionInfo.
pub(super) fn pipeline(source: &str) -> (Hir, BindingArena, RegionInfo) {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);
    (hir, arena, info)
}

/// Find the HirId of the Intrinsic node matching `op` inside a Loop body.
pub(super) fn find_intrinsic_in_loop(
    hir: &Hir,
    op: crate::hir::expr::IntrinsicOp,
) -> Option<HirId> {
    fn walk(hir: &Hir, op: crate::hir::expr::IntrinsicOp, in_loop: bool) -> Option<HirId> {
        let now_in_loop = in_loop || matches!(&hir.kind, HirKind::Loop { .. });
        if now_in_loop {
            if let HirKind::Intrinsic { op: o, .. } = &hir.kind {
                if *o == op {
                    return Some(hir.id);
                }
            }
        }
        let mut found = None;
        hir.for_each_child(|child| {
            if found.is_none() {
                found = walk(child, op, now_in_loop);
            }
        });
        found
    }
    walk(hir, op, false)
}

/// Find the HirId of an Intrinsic node matching `op` inside a Let body.
pub(super) fn find_intrinsic_in_let(hir: &Hir, op: crate::hir::expr::IntrinsicOp) -> Option<HirId> {
    fn walk(hir: &Hir, op: crate::hir::expr::IntrinsicOp, in_let: bool) -> Option<HirId> {
        let now_in_let = in_let || matches!(&hir.kind, HirKind::Let { .. });
        if now_in_let {
            if let HirKind::Intrinsic { op: o, .. } = &hir.kind {
                if *o == op {
                    return Some(hir.id);
                }
            }
        }
        let mut found = None;
        hir.for_each_child(|child| {
            if found.is_none() {
                found = walk(child, op, now_in_let);
            }
        });
        found
    }
    walk(hir, op, false)
}

pub(super) fn find_all_pairs_helper(hir: &Hir, out: &mut Vec<HirId>) {
    if let HirKind::Intrinsic { op, .. } = &hir.kind {
        if *op == crate::hir::expr::IntrinsicOp::Pair {
            out.push(hir.id);
        }
    }
    hir.for_each_child(|child| find_all_pairs_helper(child, out));
}

/// Assertion helper: verify no allocation in alloc_region is
/// assigned to a scope region that will be freed while the
/// allocation is still the body result of that scope's Let/Letrec.
///
/// This catches the fundamental defect: FreeRegion frees an
/// allocation that is part of the return value.
pub(super) fn assert_body_results_escape_scopes(info: &RegionInfo, hir: &Hir) {
    // Collect (scope_hir_id, body_hir) pairs for Let and Letrec
    fn collect_scope_bodies(hir: &Hir, out: &mut Vec<(HirId, HirId)>) {
        match &hir.kind {
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                // The body's result is the scope's result.
                // Collect the body's HirId as the "result position."
                out.push((hir.id, body.id));
            }
            _ => {}
        }
        hir.for_each_child(|child| collect_scope_bodies(child, out));
    }

    // Find whether the Hir node at `target` is a Begin or Match.
    fn is_begin_or_match(hir: &Hir, target: HirId) -> bool {
        if hir.id == target {
            return matches!(&hir.kind, HirKind::Begin(_) | HirKind::Match { .. });
        }
        let mut found = false;
        hir.for_each_child(|child| {
            if !found {
                found = is_begin_or_match(child, target);
            }
        });
        found
    }

    let mut scope_bodies = Vec::new();
    collect_scope_bodies(hir, &mut scope_bodies);

    for (scope_id, body_id) in &scope_bodies {
        let scope_r = match info.scope_region.get(scope_id) {
            Some(r) => r,
            None => continue, // no scope region (e.g., cell bindings)
        };
        // Skip phantom allocations: Begin and Match register
        // alloc_here for lowerer bookkeeping (MakeCaptureCell,
        // pattern destructuring) but aren't real heap values that
        // need to survive scope exit.
        if is_begin_or_match(hir, *body_id) {
            continue;
        }
        // If the body's allocation is in the scope region,
        // FreeRegion will free it — this is a bug when the
        // body result flows out of the scope.
        if let Some(body_r) = info.alloc_region.get(body_id) {
            if body_r == scope_r && info.live_regions.contains(scope_r) {
                panic!(
                    "body result @{} of scope @{} is in scope region r{} — \
                         FreeRegion will free it before it reaches the caller",
                    body_id.0, scope_id.0, scope_r.0
                );
            }
        }
    }
}

/// Find the HirId of the first Emit node in `hir`.
pub(super) fn find_first_emit(hir: &Hir) -> Option<HirId> {
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

/// Find the HirId of the value child of the first Emit node in `hir`.
#[allow(dead_code)]
pub(super) fn find_first_emit_value_id(hir: &Hir) -> Option<HirId> {
    if let HirKind::Emit { value, .. } = &hir.kind {
        return Some(value.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = find_first_emit_value_id(c);
        }
    });
    found
}

pub(super) fn analyze_with_hir(source: &str) -> (Hir, BindingArena, SymbolTable, RegionInfo) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);
    let wrapped = format!(
        "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
        source
    );
    let arena = crate::syntax::SyntaxArena::mint(vm.heap());
    let syntax = read_syntax(arena, &wrapped, "<test>").expect("parse");
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
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
    let info = analyze_regions(&analysis.hir, &arena);
    (analysis.hir, arena, symbols, info)
}

/// The trap these two share: a binding is found by *identity*, never by asking
/// a memo for a spelling. A primitive's id is minted against the compile
/// context's own table, so the caller's memo may never have learned the name —
/// `SymbolId::of` sidesteps that entirely (docs/impl/symbol.md § "Reading a
/// name, and not reading one").
pub(super) fn find_calls_to_primitive(hir: &Hir, name: &str, arena: &BindingArena) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, want: SymbolId, arena: &BindingArena, out: &mut Vec<HirId>) {
        if let HirKind::Call { func, .. } = &hir.kind {
            if let HirKind::Var(b) = &func.kind {
                if arena.get(*b).name == want {
                    out.push(hir.id);
                }
            }
        }
        hir.for_each_child(|c| walk(c, want, arena, out));
    }
    walk(hir, SymbolId::of(name), arena, &mut out);
    out
}

#[allow(dead_code)]
pub(super) fn find_binding_by_name(hir: &Hir, name: &str, arena: &BindingArena) -> Option<Binding> {
    fn walk(hir: &Hir, want: SymbolId, arena: &BindingArena) -> Option<Binding> {
        if let HirKind::Var(b) = &hir.kind {
            if arena.get(*b).name == want {
                return Some(*b);
            }
        }
        let mut found = None;
        hir.for_each_child(|c| {
            if found.is_none() {
                found = walk(c, want, arena);
            }
        });
        found
    }
    walk(hir, SymbolId::of(name), arena)
}

/// The `(then_id, else_id)` body HirIds of the first `If` in the tree.
pub(super) fn first_if_arms(hir: &Hir) -> Option<(HirId, HirId)> {
    if let HirKind::If {
        then_branch,
        else_branch,
        ..
    } = &hir.kind
    {
        return Some((then_branch.id, else_branch.id));
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_if_arms(c);
        }
    });
    found
}

pub(super) fn find_first<F>(hir: &Hir, pred: F) -> Option<HirId>
where
    F: Fn(&Hir) -> bool + Copy,
{
    if pred(hir) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = find_first(c, pred);
        }
    });
    found
}

pub(super) fn find_all<F>(hir: &Hir, pred: F) -> Vec<HirId>
where
    F: Fn(&Hir) -> bool + Copy,
{
    let mut out = Vec::new();
    fn walk<F>(hir: &Hir, pred: F, out: &mut Vec<HirId>)
    where
        F: Fn(&Hir) -> bool + Copy,
    {
        if pred(hir) {
            out.push(hir.id);
        }
        hir.for_each_child(|c| walk(c, pred, out));
    }
    walk(hir, pred, &mut out);
    out
}

pub(super) fn find_begins(hir: &Hir) -> Vec<HirId> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::Begin(_)) {
            out.push(hir.id);
        }
        hir.for_each_child(|c| walk(c, out));
    }
    walk(hir, &mut out);
    out
}

/// All `Assign` and `SetCell`-on-`Var` sites with their target binding.
pub(super) fn find_reassign_sites(hir: &Hir) -> Vec<(HirId, Binding)> {
    let mut out = Vec::new();
    fn walk(hir: &Hir, out: &mut Vec<(HirId, Binding)>) {
        match &hir.kind {
            HirKind::Assign { target, .. } => out.push((hir.id, *target)),
            HirKind::SetCell { cell, .. } => {
                if let HirKind::Var(b) = &cell.kind {
                    out.push((hir.id, *b));
                }
            }
            _ => {}
        }
        hir.for_each_child(|c| walk(c, out));
    }
    walk(hir, &mut out);
    out
}

/// Like `analyze_with_hir` but with the REAL primitive call
/// classification (`PrimitiveClassification::new` over the registered
/// meta), so declared region effects reach the walk.
pub(super) fn analyze_with_class(source: &str) -> (Hir, BindingArena, SymbolTable, RegionInfo) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);
    let wrapped = format!(
        "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
        source
    );
    let arena = crate::syntax::SyntaxArena::mint(vm.heap());
    let syntax = read_syntax(arena, &wrapped, "<test>").expect("parse");
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
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
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let info = analyze_regions_with(&analysis.hir, &arena, pc.call_classification);
    (analysis.hir, arena, symbols, info)
}

/// Edges recorded at one call site, as (src, dst) pairs.
pub(super) fn edges_at_site(info: &RegionInfo, site: HirId) -> Vec<(Region, Region)> {
    info.cross_region_refs
        .iter()
        .filter(|(s, _, _)| *s == site)
        .map(|&(_, src, dst)| (src, dst))
        .collect()
}

/// Like `analyze_with_class` but overriding one primitive's declared
/// effect — mechanism tests for each `RegionEffect` variant's edge shape,
/// independent of which table entries happen to be declared yet.
pub(super) fn analyze_with_effect(
    source: &str,
    prim: &str,
    effect: crate::primitives::def::RegionEffect,
) -> (Hir, BindingArena, SymbolTable, RegionInfo) {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let meta = register_primitives(&mut vm, &mut symbols);
    let wrapped = format!(
        "(letrec [cond_var (fn () nil) f (fn (& args) args) g (fn (& args) args)] {})",
        source
    );
    let arena = crate::syntax::SyntaxArena::mint(vm.heap());
    let syntax = read_syntax(arena, &wrapped, "<test>").expect("parse");
    let mut expander = Expander::on_vm(&mut vm);
    expander.set_arena(arena);
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
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let mut call_class = pc.call_classification;
    let sym = crate::value::SymbolId::of(prim);
    call_class.effects.insert(sym, effect);
    let info = analyze_regions_with(&analysis.hir, &arena, call_class);
    (analysis.hir, arena, symbols, info)
}

/// The region of the string literal with the given content.
pub(super) fn string_literal_region(hir: &Hir, info: &RegionInfo, content: &str) -> Region {
    fn walk(hir: &Hir, content: &str, out: &mut Option<HirId>) {
        if let HirKind::String(s) = &hir.kind {
            if s == content {
                *out = Some(hir.id);
            }
        }
        hir.for_each_child(|c| walk(c, content, out));
    }
    let mut id = None;
    walk(hir, content, &mut id);
    let id = id.unwrap_or_else(|| panic!("no string literal {:?}", content));
    info.alloc_region
        .get(&id)
        .copied()
        .unwrap_or_else(|| panic!("string literal {:?} has no alloc region", content))
}
