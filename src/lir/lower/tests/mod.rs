use super::*;
use crate::config::region_ownership_override::{RegionOwnership, ScopedRegionOwnership};
use crate::syntax::Span;

fn make_span() -> Span {
    Span::new(0, 0, 1, 1)
}

// ── Region-lifecycle emission tests ──────────────────────────────

/// Build a fully-configured `Lowerer` plus the analyzed HIR for `source`
/// (wrapped in the stub letrec), stopping just before `lower`. Tests that
/// inspect lowerer state (e.g. `decrefs_by_decref_point`) use this directly;
/// `compile_to_lir` drives it through `lower`.
fn make_lowerer(source: &str) -> (Lowerer<'static>, crate::hir::Hir) {
    make_lowerer_with(source, |_, _| {})
}

/// Like [`make_lowerer`], but lets the caller MUTATE the computed `RegionInfo`
/// before it reaches the Lowerer — the injection seam for emit-contract tests
/// that need an edge the inference does not produce for any shape (an upvalue
/// capture-adopt edge: a region-rooted upvalue owner is refused on the lifetime
/// obligation, and the admitting owner is the activation/fiber node — see
/// region-model.md § "The capture adopt").
fn make_lowerer_with(
    source: &str,
    mutate: impl FnOnce(&mut crate::hir::region::RegionInfo, &crate::hir::Hir),
) -> (Lowerer<'static>, crate::hir::Hir) {
    use crate::hir::functionalize::functionalize;
    use crate::hir::tailcall::mark_tail_calls;
    use crate::hir::Analyzer;
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

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
    let arena = Box::leak(Box::new(crate::hir::BindingArena::new()));
    let mut analyzer = Analyzer::new(&mut symbols, arena);
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded).expect("analyze");
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);
    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, arena);
    crate::hir::anf::anf_lift(&mut analysis.hir, arena);

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let mut region_info =
        crate::hir::analyze_regions_with(&analysis.hir, arena, pc.call_classification.clone());
    mutate(&mut region_info, &analysis.hir);
    let lowerer = Lowerer::new(arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbols.all_names())
        .with_region_info(region_info);
    (lowerer, analysis.hir)
}

fn compile_to_lir(source: &str) -> crate::lir::LirModule {
    let (mut lowerer, hir) = make_lowerer(source);
    lowerer.lower(&hir).expect("lower")
}

fn count_decref_regions(module: &crate::lir::LirModule) -> usize {
    fn count_in_func(func: &LirFunction) -> usize {
        func.blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|i| matches!(i.instr, LirInstr::DecrefRegion { .. }))
            .count()
    }
    count_in_func(&module.entry) + module.closures.iter().map(count_in_func).sum::<usize>()
}

fn count_decref_value_regions(module: &crate::lir::LirModule) -> usize {
    fn count_in_func(func: &LirFunction) -> usize {
        func.blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|i| matches!(i.instr, LirInstr::DecrefValueRegion { .. }))
            .count()
    }
    count_in_func(&module.entry) + module.closures.iter().map(count_in_func).sum::<usize>()
}

fn count_adopt_regions(module: &crate::lir::LirModule) -> usize {
    fn count_in_func(func: &LirFunction) -> usize {
        func.blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|i| matches!(i.instr, LirInstr::AdoptRegion { .. }))
            .count()
    }
    count_in_func(&module.entry) + module.closures.iter().map(count_in_func).sum::<usize>()
}

fn count_load_self(module: &crate::lir::LirModule) -> usize {
    fn count_in_func(func: &LirFunction) -> usize {
        func.blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .filter(|i| matches!(i.instr, LirInstr::LoadSelf { .. }))
            .count()
    }
    count_in_func(&module.entry) + module.closures.iter().map(count_in_func).sum::<usize>()
}

/// The canonical captured-non-reassigned-mutable shape
/// (region-capture-cell-noreassign-uaf.lisp): `@acc` is boxed in a
/// `MakeCaptureCell` (captured by closures, never reassigned); its init is an
/// opaque call result released by `LoadLocal(slot) + DecrefValueRegion`
/// (which unwraps the cell); the cell's own region is released by a plain
/// `DecrefRegion` at the same decref_point.
const CAPTURE_CELL_SHAPE: &str = "(begin \
     (def @acc (f 1 2 3)) \
     (def u1 (fn () acc)) (def u2 (fn () acc)) (def u3 (fn () acc)) \
     (def u4 (fn () acc)) (def u5 (fn () acc)) \
     (g (u1)) (g (u5)))";

/// Rewrite every `StaticRegion(N)` in a compact-debug LIR dump to its
/// first-appearance rank. Static region ids come from a process-global
/// counter (`new_static_region`), so absolute ids legitimately differ
/// between two compiles of the same source; the *order in which slots are
/// minted and used* is what must be deterministic.
fn canonicalize_static_regions(s: &str) -> String {
    let mut ranks: HashMap<u64, usize> = HashMap::new();
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    const NEEDLE: &str = "StaticRegion(";
    while let Some(pos) = rest.find(NEEDLE) {
        let num_start = pos + NEEDLE.len();
        out.push_str(&rest[..num_start]);
        rest = &rest[num_start..];
        let num_len = rest.find(')').expect("unclosed StaticRegion(");
        let id: u64 = rest[..num_len]
            .trim()
            .trim_end_matches(',')
            .parse()
            .expect("StaticRegion id");
        let next = ranks.len();
        let rank = *ranks.entry(id).or_insert(next);
        out.push_str(&format!("#{rank}"));
        rest = &rest[num_len..];
    }
    out.push_str(rest);
    out
}

/// Pointers to every `HirKind::Return`'s value child — the node `lower_return`
/// (and so the coalescing predicate) receives. Raw pointers because
/// `for_each_child`'s closure cannot extend the borrow; the HIR outlives the
/// vector at every call site.
fn return_value_ptrs(hir: &Hir) -> Vec<*const Hir> {
    fn go(h: &Hir, out: &mut Vec<*const Hir>) {
        if let HirKind::Return { value } = &h.kind {
            out.push(&**value as *const Hir);
        }
        h.for_each_child(|c| go(c, out));
    }
    let mut out = Vec::new();
    go(hir, &mut out);
    out
}

/// A function's instructions across all blocks, flattened in emission order —
/// for order-sensitive region-RC assertions.
fn flat_instrs(func: &LirFunction) -> Vec<&LirInstr> {
    func.blocks
        .iter()
        .flat_map(|b| b.instructions.iter())
        .map(|si| &si.instr)
        .collect()
}

fn func_count(func: &LirFunction, pred: impl Fn(&LirInstr) -> bool) -> usize {
    flat_instrs(func).into_iter().filter(|i| pred(i)).count()
}

/// True if any of the function's instructions names a static region (an
/// allocation or a per-call routing region — `LirInstr::region`). A function with
/// none allocates nothing, so it has no fresh local region a return could coalesce
/// onto — its tail mint must stay value-resolved.
fn allocates_or_calls(func: &LirFunction) -> bool {
    flat_instrs(func).into_iter().any(|i| i.region().is_some())
}

/// Pin the C0 emit contract at a coalesced site: under `debug_assertions` the
/// lowerer emits `AssertRegionMatches { region_id }` immediately before the
/// coalesced `IncrefRegion { region_id }`, naming the SAME slot; in release it
/// emits no oracle at all.
fn assert_coalesced_oracle_precedes(func: &LirFunction) {
    let instrs = flat_instrs(func);
    let inc_pos = instrs
        .iter()
        .position(|i| matches!(i, LirInstr::IncrefRegion { .. }))
        .expect("a coalesced IncrefRegion");
    let inc_slot = match instrs[inc_pos] {
        LirInstr::IncrefRegion { region_id } => *region_id,
        _ => unreachable!(),
    };
    if cfg!(debug_assertions) {
        match inc_pos.checked_sub(1).map(|p| instrs[p]) {
            Some(LirInstr::AssertRegionMatches { region_id, .. }) => assert_eq!(
                *region_id, inc_slot,
                "the oracle must guard the SAME slot the coalesced IncrefRegion uses",
            ),
            other => panic!(
                "debug builds must emit AssertRegionMatches immediately before the \
                 coalesced IncrefRegion; found {other:?}",
            ),
        }
    } else {
        assert!(
            !instrs
                .iter()
                .any(|i| matches!(i, LirInstr::AssertRegionMatches { .. })),
            "release builds must not emit the debug-only AssertRegionMatches",
        );
    }
}

const BUILDER_IDIOM: &str = "(begin (%pair (%pair 1 2) 3) nil)";

/// The static region slots the builder idiom's two `%pair` allocations
/// (`LirInstr::List`) are emitted against, in the entry function. The discarded
/// nested literal is the only source of `List` in the entry (the letrec stub
/// lambdas are `MakeClosure`s; their rest-arg bodies live in closures).
fn builder_pair_slots(module: &crate::lir::LirModule) -> Vec<StaticRegion> {
    flat_instrs(&module.entry)
        .into_iter()
        .filter_map(|i| match i {
            LirInstr::List { region, .. } => Some(*region),
            _ => None,
        })
        .collect()
}

// ── Themed test submodules ───────────────────────────────────────
mod basics;
mod coalesce;
mod merge;
mod release;
