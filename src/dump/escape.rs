//! The `escape` dump kind — a normalized, deterministic snapshot of the
//! escape-relevant compiler facts and the region instructions they drive.
//!
//! Unlike the other dump bodies (which mirror `elle --dump=KIND`'s human
//! rendering, complete with the absolute `@`-HirIds that make them
//! non-deterministic across compiles — docs/test-runner.md), this body is
//! **id-normalized** so two compiles of the same source render byte-identically.
//! That makes it the input to the escape golden (`tests/elle/escape-golden.lisp`).
//! Escape (`hir/escape.rs`) is the single authority for whether a value escapes; the
//! region solver, `functionalize`, and the lowerer's tail-call predicates read it
//! rather than recomputing a proxy. The snapshot records that surface — escape's
//! verdict projected to regions (`[return_frontier]`) and the RC instructions it
//! drives (`[region_instrs]`) — so a change to escape or its consumers is visible as
//! a snapshot diff to be reviewed (the emitted RC may *tighten* as escape's precision
//! lands; it must never coarsen or introduce a UAF/leak).
//!
//! ## Sections (the escape producer/consumer surface)
//!
//! - `[needs_capture]` — per program binding: the cell-layout decision
//!   `needs_capture()` (consumed by `functionalize` and `lambda.rs`) and its
//!   independent structural inputs (`mutated`/`immutable`/scope). The
//!   lexical-capture proxy `is_captured` is module-private with no escape
//!   authority, so it is not rendered — `needs_capture()` is the surface that
//!   matters here.
//! - `[lambda_captures]` — per lambda: its params and capture set (`CaptureInfo`).
//! - `[return_frontier]` — the regions escape's authoritative return verdict
//!   projects onto (`crate::hir::EscapeInfo`, projected through `alloc_region` /
//!   `binding_source_regions` by `regions::escape`).
//! - `[suppressed_decref_regions]` — the reassign-gate / store-path-owned set.
//! - `[region_instrs]` — per LIR function, the emitted RC instructions
//!   (`Incref/DecrefRegion`, `Incref/DecrefValueRegion`, `DecrefCellRegion`) and
//!   each tail call's `adopt_callee` flag — the behavior the facts above produce.
//!
//! ## Normalization (why it is freezable)
//!
//! - **`HirId`** (process-global counter) → `#n` by pre-order `for_each_child`
//!   walk.
//! - **`StaticRegion`** (global `NEXT_STATIC_REGION`) → `s<k>` by first
//!   appearance in the emitted stream.
//! - **`Region`** (per-compilation) → `r<k>` by first appearance over the walk's
//!   allocation sites, then remaining fact-set regions in raw-id order.
//! - **`Binding`** → `b<n>` by dense first-appearance over the program's own HIR
//!   bindings, *not* the raw arena index (the arena also holds compile-time-env
//!   stdlib bindings whose count shifts every program's indices when the prelude
//!   grows). Unreferenced stdlib names and primitives are dropped.
//! - Register operands stay raw (`v<n>`): `Reg` is a per-function counter reset
//!   per `lower`, already run-stable.
//!
//! The region info handed in is computed with the real `PrimitiveClassification`
//! (see `render_all`) — not the bare `analyze_regions` — so the facts match what
//! the lowerer consumed.

use std::collections::HashMap;
use std::fmt::Write;

use crate::hir::region::{Region, RegionInfo, StaticRegion};
use crate::hir::{Binding, BindingArena, CaptureKind, EscapeInfo, Hir, HirId, HirKind};
use crate::lir::{LirFunction, LirInstr, LirModule};

type Names = HashMap<u32, String>;

/// Render the normalized escape snapshot for a compiled module.
pub fn escape_module(
    hir: &Hir,
    arena: &BindingArena,
    names: &Names,
    escape: &EscapeInfo,
    ri: &RegionInfo,
    module: &LirModule,
) -> String {
    // The return frontier — escape's authoritative return verdict projected onto
    // regions. A flat region set; its members feed both the region normalization
    // and the `[return_frontier]` section below.
    let return_frontier =
        crate::hir::return_frontier_regions(escape, &ri.alloc_region, &ri.binding_source_regions);
    // One deterministic pre-order walk yields both id-spaces. `HirId` → `#n`
    // (process-global counter; must be anchored). `Binding` → `b<n>` by dense
    // first-appearance, NOT raw arena index: the arena also holds compile-time-env
    // stdlib bindings whose count shifts every program's indices when the prelude
    // grows. Only bindings that appear in the program's HIR are collected, and
    // primitives are skipped (never captured/mutated — pure noise).
    let mut preorder: Vec<HirId> = Vec::new();
    let mut binding_order: Vec<Binding> = Vec::new();
    let mut seen: std::collections::HashSet<Binding> = std::collections::HashSet::new();
    collect_norm(hir, arena, &mut preorder, &mut binding_order, &mut seen);
    let hir_norm: HashMap<HirId, usize> = preorder
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    let binding_norm: HashMap<Binding, usize> = binding_order
        .iter()
        .enumerate()
        .map(|(i, b)| (*b, i))
        .collect();
    // Per-binding DISPLAY name, with the gensym counter normalized away. A
    // gensym name is `<prefix><GENSYM_COUNTER>` (src/primitives/meta.rs) — that
    // trailing global counter leaks the process-wide allocation history into the
    // name (`G26` here, `G12` there), so it is renumbered to a first-appearance
    // index per base (`G26 G27` → `G#0 G#1`), keyed off the deterministic walk
    // order so two contexts that mint different raw counters still render
    // identically. Names without a trailing-digit suffix (`x`, `down`, `~`) are
    // left verbatim. Hygiene does not depend on the counter (it is carried by
    // scopes), so this is display-only.
    let name_label = build_name_labels(&binding_order, arena, names);
    let h = |id: HirId| -> String {
        match hir_norm.get(&id) {
            Some(n) => format!("#{n}"),
            None => format!("#?{}", id.0),
        }
    };

    // Region → r<k>: alloc sites in walk order first, then remaining fact-set
    // regions in raw-id order, so the numbering never depends on map iteration.
    let mut region_norm: HashMap<Region, usize> = HashMap::new();
    let mut next_r = 0usize;
    let see_region = |r: Region, m: &mut HashMap<Region, usize>, n: &mut usize| {
        m.entry(r).or_insert_with(|| {
            let k = *n;
            *n += 1;
            k
        });
    };
    for id in &preorder {
        if let Some(r) = ri.alloc_region.get(id) {
            see_region(*r, &mut region_norm, &mut next_r);
        }
    }
    let mut leftover: Vec<Region> = Vec::new();
    leftover.extend(return_frontier.iter().copied());
    leftover.extend(ri.suppressed_decref_regions.iter().copied());
    leftover.sort_by_key(|r| r.0);
    for r in leftover {
        see_region(r, &mut region_norm, &mut next_r);
    }
    let rr = |r: Region| -> String {
        match region_norm.get(&r) {
            Some(k) => format!("r{k}"),
            None => format!("r?{}", r.0),
        }
    };

    let mut s = String::new();

    // [needs_capture] — the program's own bindings, in first-appearance order.
    let _ = writeln!(s, "[needs_capture]");
    for b in &binding_order {
        let bi = arena.get(*b);
        let scope = match bi.scope {
            crate::hir::BindingScope::Parameter => "param",
            crate::hir::BindingScope::Local => "local",
        };
        let _ = writeln!(
            s,
            "  {} {} mutated={} immutable={} needs_capture={}",
            blabel(*b, &binding_norm, &name_label),
            scope,
            bi.is_mutated,
            bi.is_immutable,
            bi.needs_capture(),
        );
    }

    // [lambda_captures] — per lambda, in walk order.
    let _ = writeln!(s, "[lambda_captures]");
    let mut lambdas: Vec<LamInfo> = Vec::new();
    collect_lambdas(hir, &mut lambdas);
    for lam in &lambdas {
        let mut ps: Vec<String> = lam
            .params
            .iter()
            .map(|b| blabel(*b, &binding_norm, &name_label))
            .collect();
        if let Some(rp) = lam.rest {
            ps.push(format!("&rest {}", blabel(rp, &binding_norm, &name_label)));
        }
        let caps: Vec<String> = lam
            .caps
            .iter()
            .map(|(b, k)| {
                // Render the kind WITHOUT the raw `Binding` a `Recursive` variant carries:
                // that index is non-normalized and would make the golden non-deterministic
                // (it is the same binding `blabel` already prints). `Local`/`Capture` keep
                // their `{:?}` form, so existing snapshots are unchanged.
                let kstr = match k {
                    CaptureKind::Recursive { .. } => "Recursive".to_string(),
                    other => format!("{other:?}"),
                };
                format!("{}<{}>", blabel(*b, &binding_norm, &name_label), kstr)
            })
            .collect();
        let _ = writeln!(
            s,
            "  {} params=[{}] captures=[{}]",
            h(lam.id),
            ps.join(", "),
            caps.join(", "),
        );
    }

    // [return_frontier] — the regions escape's authoritative return verdict
    // projects onto (a flat set sourced from `crate::hir::EscapeInfo`, not a
    // solver-local fact).
    let _ = writeln!(s, "[return_frontier]");
    let mut rf: Vec<String> = return_frontier.iter().map(|r| rr(*r)).collect();
    rf.sort();
    let _ = writeln!(s, "  [{}]", rf.join(", "));

    // [suppressed_decref_regions] — sorted by normalized region id.
    let _ = writeln!(s, "[suppressed_decref_regions]");
    let mut sup: Vec<String> = ri
        .suppressed_decref_regions
        .iter()
        .map(|r| rr(*r))
        .collect();
    sup.sort();
    let _ = writeln!(s, "  [{}]", sup.join(", "));

    // [region_instrs] — per function, the emitted RC ops + tail adopt flags.
    let _ = writeln!(s, "[region_instrs]");
    let mut lir_region_norm: HashMap<StaticRegion, usize> = HashMap::new();
    let mut next_s = 0usize;
    render_func_region_instrs(
        &mut s,
        "entry",
        &module.entry,
        &mut lir_region_norm,
        &mut next_s,
    );
    for (i, f) in module.closures.iter().enumerate() {
        render_func_region_instrs(
            &mut s,
            &format!("closure[{i}]"),
            f,
            &mut lir_region_norm,
            &mut next_s,
        );
    }

    s
}

/// Append one function's region-relevant instruction stream. Non-RC instructions
/// are elided; block labels are kept so a moved RC op is visible.
fn render_func_region_instrs(
    s: &mut String,
    tag: &str,
    f: &LirFunction,
    sn: &mut HashMap<StaticRegion, usize>,
    next_s: &mut usize,
) {
    let name = f.name.as_deref().unwrap_or("<anon>");
    let _ = writeln!(s, "  ; {tag} {name}");
    let sreg = |r: StaticRegion, m: &mut HashMap<StaticRegion, usize>, n: &mut usize| -> String {
        let k = *m.entry(r).or_insert_with(|| {
            let k = *n;
            *n += 1;
            k
        });
        format!("s{k}")
    };
    for block in &f.blocks {
        for si in &block.instructions {
            let line = match &si.instr {
                LirInstr::IncrefRegion { region_id } => {
                    format!("IncrefRegion {}", sreg(*region_id, sn, next_s))
                }
                LirInstr::DecrefRegion { region_id } => {
                    format!("DecrefRegion {}", sreg(*region_id, sn, next_s))
                }
                LirInstr::IncrefValueRegion { src } => format!("IncrefValueRegion v{}", src.0),
                LirInstr::DecrefValueRegion { src } => format!("DecrefValueRegion v{}", src.0),
                LirInstr::DecrefCellRegion { src } => format!("DecrefCellRegion v{}", src.0),
                LirInstr::TailCall { adopt_callee, .. } => {
                    format!("TailCall adopt_callee={adopt_callee}")
                }
                LirInstr::TailCallArrayMut { .. } => "TailCallArrayMut".to_string(),
                _ => continue,
            };
            let _ = writeln!(s, "    {}: {}", block.label, line);
        }
    }
}

fn blabel(
    b: Binding,
    bnorm: &HashMap<Binding, usize>,
    name_label: &HashMap<Binding, String>,
) -> String {
    let name = name_label.get(&b).map(String::as_str).unwrap_or("?");
    match bnorm.get(&b) {
        Some(n) => format!("b{n}({name})"),
        None => format!("b?{}({name})", b.0),
    }
}

/// Build each binding's normalized display name. Gensym names carry a trailing
/// process-global counter (`G26`); strip it and renumber per base by
/// first-appearance over `order` (the deterministic walk), so the label is
/// independent of how many gensyms were minted before this compile. Names with no
/// trailing digit run are kept verbatim (`x`, `down`, the synthetic `~`).
fn build_name_labels(
    order: &[Binding],
    arena: &BindingArena,
    names: &Names,
) -> HashMap<Binding, String> {
    let mut out: HashMap<Binding, String> = HashMap::new();
    let mut by_raw: HashMap<String, String> = HashMap::new();
    let mut base_ctr: HashMap<String, usize> = HashMap::new();
    for &b in order {
        let raw = names
            .get(&arena.get(b).name.0)
            .map(String::as_str)
            .unwrap_or("~")
            .to_string();
        let label = by_raw.entry(raw.clone()).or_insert_with(|| {
            let base = raw.trim_end_matches(|c: char| c.is_ascii_digit());
            if base.len() == raw.len() || base.is_empty() {
                // no trailing digits (or all-digits, which a symbol never is)
                raw.clone()
            } else {
                let idx = base_ctr.entry(base.to_string()).or_insert(0);
                let label = format!("{base}#{idx}");
                *idx += 1;
                label
            }
        });
        out.insert(b, label.clone());
    }
    out
}

/// One pre-order walk collecting both the `HirId` order (the structural
/// traversal — `for_each_child`'s execution order, the natural anchor for the
/// facts) and the first-appearance order of the program's own (non-primitive)
/// bindings. A binding is "noted" at the node that introduces or references it —
/// definitions (Let/Letrec/Loop targets, Lambda params/rest/captures,
/// Assign/Define targets) and plain reads (Var). Pattern bindings
/// (Match/Destructure) are caught when read as a `Var`; one never read is dead
/// and dropped, which is correct.
fn collect_norm(
    hir: &Hir,
    arena: &BindingArena,
    pre: &mut Vec<HirId>,
    order: &mut Vec<Binding>,
    seen: &mut std::collections::HashSet<Binding>,
) {
    pre.push(hir.id);
    let note =
        |b: Binding, order: &mut Vec<Binding>, seen: &mut std::collections::HashSet<Binding>| {
            if !arena.get(b).is_primitive && seen.insert(b) {
                order.push(b);
            }
        };
    match &hir.kind {
        HirKind::Var(b) => note(*b, order, seen),
        HirKind::Let { bindings, .. }
        | HirKind::Letrec { bindings, .. }
        | HirKind::Loop { bindings, .. } => {
            for (b, _) in bindings {
                note(*b, order, seen);
            }
        }
        HirKind::Lambda {
            params,
            rest_param,
            captures,
            ..
        } => {
            for p in params {
                note(*p, order, seen);
            }
            if let Some(rp) = rest_param {
                note(*rp, order, seen);
            }
            for c in captures {
                note(c.binding, order, seen);
            }
        }
        HirKind::Assign { target, .. } => note(*target, order, seen),
        HirKind::Define { binding, .. } => note(*binding, order, seen),
        _ => {}
    }
    hir.for_each_child(|c| collect_norm(c, arena, pre, order, seen));
}

/// Owned per-lambda facts — `for_each_child` hands out short-lived `&Hir`, so we
/// extract what the snapshot needs rather than retaining borrows of the tree.
struct LamInfo {
    id: HirId,
    params: Vec<Binding>,
    rest: Option<Binding>,
    caps: Vec<(Binding, CaptureKind)>,
}

fn collect_lambdas(hir: &Hir, out: &mut Vec<LamInfo>) {
    if let HirKind::Lambda {
        params,
        rest_param,
        captures,
        ..
    } = &hir.kind
    {
        out.push(LamInfo {
            id: hir.id,
            params: params.clone(),
            rest: *rest_param,
            caps: captures.iter().map(|c| (c.binding, c.kind)).collect(),
        });
    }
    hir.for_each_child(|c| collect_lambdas(c, out));
}
