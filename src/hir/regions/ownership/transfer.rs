//! The transferred-returned-subtree cut: a callee-built, externally-unique
//! subtree containing a reference cycle, handed to its consumer across the
//! return (or fiber-terminal) frontier and owned by the **consuming
//! activation** (docs/impl/region/owner.md § "Owner nodes" — "The transferred
//! returned subtree").
//!
//! Inside the producer the root crosses the return frontier, so every
//! region-rooted mode refuses (a Shared seed poisons the subtree and group
//! walks); in the consumer the root is an opaque call-result whose
//! `DecrefValueRegion` releases one reference while the cycle's interior
//! back-edge holds another — the cycle survives every release and leaks per
//! call. The consuming activation's owner node reclaims it: its completion
//! release post-dominates every use of the result on either side of the
//! frontier, so replacing the consumer's release with `AdoptIntoActivation`
//! (consuming the whole count, stuck back-edge reference included) lets the
//! node's set-drop reclaim root + members wholesale.

use super::super::escape::captured_bindings;
use super::super::*;
use super::capture::capture_containment_edges;
use super::inputs::ownership_inputs;
use super::seeds::compute_shared_seeds;
use rustc_hash::{FxHashMap, FxHashSet};

/// The transfer cut's output, computed by the ownership pass in `analyze_regions_with`.
/// The interior owner edges are merged into the ordinary adopt maps
/// (`RegionInfo::owned_adopt_edges` / `capture_adopt_edges` — same emission,
/// same suppress ⊆ adopt contract for capture members); `result_regions` are
/// the consumer-site call-result regions whose release the lowerer replaces
/// with `AdoptIntoActivation` (`RegionInfo::transfer_adopt_regions`).
pub(in crate::hir::regions) struct TransferAdopts {
    /// Emit-site HirId (a store site, or a funnel call site — the funnel store
    /// face, where the adopt is value-resolved and needs no store opcode) →
    /// interior `(member, owner)` adopts.
    pub store: HashMap<HirId, Vec<(Region, Region)>>,
    /// Closure-construction HirId → interior `(captured member, closure)` adopts.
    pub capture: HashMap<HirId, Vec<(Region, Region)>>,
    /// Consumer-site call-result regions to release by `AdoptIntoActivation`.
    pub result_regions: FxHashSet<Region>,
}

/// How a `Var(b)` occurrence is consumed — the use-form classification the
/// producer/consumer gates read. Everything the gates cannot prove harmless is
/// `Other` (a bare read: an alias, a store operand, a return, an intrinsic
/// argument — each a route a reference could escape the cut's reclamation
/// horizon through).
#[derive(Clone, Copy, PartialEq, Eq)]
enum UseForm {
    /// The callee position of a call: `(f …)`.
    Callee(HirId),
    /// Argument 0 of a `fiber/new` call (the body closure).
    FiberNewArg0(HirId),
    /// Argument 0 of a `fiber/resume` call (the fiber being driven).
    ResumeArg0(HirId),
    /// An argument of a native whose declared `RegionEffect` is `Immediate` —
    /// the result is an immediate and no argument is stored or aliased, so the
    /// read is harmless (`fiber/status`, `fiber/set-fuel`).
    ImmediateArg,
    /// Any other occurrence.
    Other,
}

/// Per-unit facts the gates read repeatedly: every `Var` occurrence classified
/// by use form, each call site's innermost enclosing Lambda, and each
/// binding's inits. Child nodes are stored as `*const Hir` — the established
/// idiom for keeping HIR references across a `for_each_child` walk (the tree
/// outlives the analysis; see `RegionInference::binding_lambda`).
struct UseIndex {
    /// Binding → its classified occurrences, in structural walk order.
    uses: FxHashMap<Binding, Vec<UseForm>>,
    /// Binding → EVERY init expression a Let/Letrec/Define/Destructure gives
    /// it, in walk order. A binding is a producer / fiber-body candidate only
    /// when it has exactly one init and that init is a Lambda — a sibling
    /// re-def or a destructure target has no stable binding→lambda identity.
    inits: FxHashMap<Binding, Vec<*const Hir>>,
    /// Call-node → innermost enclosing Lambda HirId (`None` = the top level).
    enclosing: FxHashMap<HirId, Option<HirId>>,
    /// Call-node → the bindings bound (directly, or through the ANF producer
    /// wrapper) to that call's value. A call with no entry was consumed at a
    /// non-binding position: a statement discard (safe), or a tail (a
    /// region-level return seed the consumer gate refuses independently).
    bound_to: FxHashMap<HirId, Vec<Binding>>,
}

/// The declared effect of a call's callee, under the same immutable-unshadowed
/// condition the region walk applies (`RegionInference::call_effect`).
fn callee_effect(
    func: &Hir,
    arena: &BindingArena,
    cc: &CallClassification,
) -> Option<crate::primitives::def::RegionEffect> {
    callee_symbol(func, arena).and_then(|sym| cc.effects.get(&sym).copied())
}

/// The callee's SymbolId, when it is an immutable, never-mutated binding.
fn callee_symbol(func: &Hir, arena: &BindingArena) -> Option<crate::value::SymbolId> {
    if let HirKind::Var(b) = &unwrap_cell(func).kind {
        let bi = arena.get(*b);
        if bi.is_immutable && !bi.is_mutated {
            return Some(bi.name);
        }
    }
    None
}

/// Descend the structural/ANF wrappers to the expression a position actually
/// consumes: the ANF lift names an allocating argument in place —
/// `(fiber/new (fn …) mask)` becomes `(fiber/new (let [t (fn …)] t) mask)` — so
/// the value at an arg position sits at the wrapper's tail.
fn anf_tail(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => anf_tail(body),
        HirKind::Begin(es) => es.last().map_or(h, anf_tail),
        _ => h,
    }
}

/// Unwrap the cell wrappers a captured binding's flow wears: `MakeCell` around
/// its init, `DerefCell` around each read (both lowerer-transparent — the value
/// identity is unchanged). A captured producer's Define init and its call-site
/// reads must resolve through them.
fn unwrap_cell(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::MakeCell { value } => unwrap_cell(value),
        HirKind::DerefCell { cell } => unwrap_cell(cell),
        _ => h,
    }
}

impl UseIndex {
    fn build(hir: &Hir, arena: &BindingArena, cc: &CallClassification) -> Self {
        let mut ix = UseIndex {
            uses: FxHashMap::default(),
            inits: FxHashMap::default(),
            enclosing: FxHashMap::default(),
            bound_to: FxHashMap::default(),
        };
        walk(hir, arena, cc, None, &mut ix);
        return ix;

        fn classify_arg(
            call: &Hir,
            func: &Hir,
            index: usize,
            arena: &BindingArena,
            cc: &CallClassification,
        ) -> UseForm {
            let sym = callee_symbol(func, arena);
            if index == 0 {
                if sym.is_some() && sym == cc.fiber_new {
                    return UseForm::FiberNewArg0(call.id);
                }
                if sym.is_some() && sym == cc.fiber_resume {
                    return UseForm::ResumeArg0(call.id);
                }
            }
            if callee_effect(func, arena, cc)
                == Some(crate::primitives::def::RegionEffect::Immediate)
            {
                return UseForm::ImmediateArg;
            }
            UseForm::Other
        }

        /// Record a binding's init: the init list (the binding→lambda
        /// identity) and, when the init's value is a call, the call→binding
        /// edge the consumer gate reads (`bound_to`).
        fn record_binding(ix: &mut UseIndex, b: Binding, init: &Hir) {
            ix.inits.entry(b).or_default().push(init as *const Hir);
            let tail = anf_tail(unwrap_cell(init));
            if matches!(tail.kind, HirKind::Call { .. }) {
                ix.bound_to.entry(tail.id).or_default().push(b);
            }
        }

        /// Walk an argument expression and return the binding its value
        /// position consumes: a bare `Var`, or the tail `Var` of an ANF
        /// producer wrapper (whose inits and non-tail statements are walked
        /// normally — only the tail read itself is classified by the caller
        /// instead of recorded as `Other`). `None` for any other tail, which
        /// is walked in full.
        fn arg_value_binding(
            h: &Hir,
            arena: &BindingArena,
            cc: &CallClassification,
            lambda: Option<HirId>,
            ix: &mut UseIndex,
        ) -> Option<Binding> {
            match &unwrap_cell(h).kind {
                HirKind::Var(b) => Some(*b),
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (b, init) in bindings {
                        record_binding(ix, *b, init);
                        walk(init, arena, cc, lambda, ix);
                    }
                    arg_value_binding(body, arena, cc, lambda, ix)
                }
                HirKind::Begin(es) => {
                    let (last, rest) = es.split_last()?;
                    for e in rest {
                        walk(e, arena, cc, lambda, ix);
                    }
                    arg_value_binding(last, arena, cc, lambda, ix)
                }
                _ => {
                    walk(h, arena, cc, lambda, ix);
                    None
                }
            }
        }

        fn walk(
            h: &Hir,
            arena: &BindingArena,
            cc: &CallClassification,
            lambda: Option<HirId>,
            ix: &mut UseIndex,
        ) {
            match &h.kind {
                HirKind::Var(b) => {
                    // A bare Var reached structurally (not intercepted by the
                    // Call arm below) is an unclassified read.
                    ix.uses.entry(*b).or_default().push(UseForm::Other);
                }
                HirKind::Call { func, args, .. } => {
                    ix.enclosing.insert(h.id, lambda);
                    match &unwrap_cell(func).kind {
                        HirKind::Var(b) => {
                            ix.uses.entry(*b).or_default().push(UseForm::Callee(h.id))
                        }
                        _ => walk(func, arena, cc, lambda, ix),
                    }
                    for (i, arg) in args.iter().enumerate() {
                        // The binding the arg position consumes, descending the
                        // ANF producer wrapper (whose inits/effects are walked
                        // normally); a non-binding tail is just walked.
                        if let Some(a) = arg_value_binding(&arg.expr, arena, cc, lambda, ix) {
                            let form = classify_arg(h, func, i, arena, cc);
                            ix.uses.entry(a).or_default().push(form);
                        }
                    }
                }
                HirKind::Lambda { body, .. } => {
                    // A capture is not a Var occurrence; only the body's reads
                    // count (captured bindings are gated separately through
                    // the structural capture sets).
                    walk(body, arena, cc, Some(h.id), ix);
                }
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (b, init) in bindings {
                        record_binding(ix, *b, init);
                        walk(init, arena, cc, lambda, ix);
                    }
                    // The ANF producer wrapper's self-tail read
                    // (`(let [t e] t)`) is pure value flow — the position the
                    // LET occupies is what consumes the value (an arg position
                    // classifies it through `arg_value_binding`; a statement
                    // discards it; a returned value is a region-level seed).
                    // Recording it as a read would mark every ANF-named value
                    // `Other` and refuse everything.
                    if let (1, HirKind::Var(v)) = (bindings.len(), &body.kind) {
                        if *v == bindings[0].0 {
                            return;
                        }
                    }
                    walk(body, arena, cc, lambda, ix);
                }
                HirKind::Define { binding, value } => {
                    record_binding(ix, *binding, value);
                    walk(value, arena, cc, lambda, ix);
                }
                HirKind::Destructure { pattern, value, .. } => {
                    // A destructure target's value identity is not a lambda
                    // init — record the (non-lambda) source as a poisoning
                    // init for each bound name.
                    for b in pattern.bindings().bindings {
                        ix.inits
                            .entry(b)
                            .or_default()
                            .push(value.as_ref() as *const Hir);
                    }
                    walk(value, arena, cc, lambda, ix);
                }
                _ => {
                    h.for_each_child(|c| walk(c, arena, cc, lambda, ix));
                }
            }
        }
    }

    /// The binding's sole Lambda init, when it is immutable, never mutated,
    /// and has exactly one recorded init that is a Lambda — the stable
    /// binding→lambda identity every gate below leans on.
    ///
    /// SAFETY of the deref: the pointers index nodes of the HIR tree, which
    /// outlives this analysis (both live for the `analyze_regions_with` call).
    fn sole_lambda_init(&self, b: Binding, arena: &BindingArena) -> Option<&Hir> {
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return None;
        }
        match self.inits.get(&b).map(|v| v.as_slice()) {
            Some([l]) => {
                let l = unwrap_cell(unsafe { &**l });
                matches!(l.kind, HirKind::Lambda { .. }).then_some(l)
            }
            _ => None,
        }
    }
}

/// One producer summary: interior owner edges by emit site and kind, plus the
/// returned subtree's root.
struct Summary {
    root: Region,
    /// `(emit site, member, owner)` — store/funnel-site edges.
    store_edges: Vec<(HirId, Region, Region)>,
    /// `(closure-construction site, member, owner)` — capture edges.
    capture_edges: Vec<(HirId, Region, Region)>,
}

/// Compute the transfer cut (docs/impl/region/owner.md § "Owner nodes" — "The
/// transferred returned subtree"). Producer and consumer halves are admitted
/// only together: the interior adopts freeze member counts, so a consumer that
/// could alias a member out of the node's reclamation horizon refuses the
/// whole callee — one inadmissible site refuses every site.
pub(in crate::hir::regions) fn compute_transfer_adopts(
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    arena: &BindingArena,
    call_class: &CallClassification,
    order: &HashMap<HirId, u32>,
) -> TransferAdopts {
    let mut out = TransferAdopts {
        store: HashMap::new(),
        capture: HashMap::new(),
        result_regions: FxHashSet::default(),
    };
    let inputs = ownership_inputs(hir, info, escape, arena);
    let shared = compute_shared_seeds(info, escape);
    let capture_edges = capture_containment_edges(hir, info, arena);
    let captured = captured_bindings(hir);
    let ix = UseIndex::build(hir, arena, call_class);
    let low = compute_subtree_low(hir, order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    // Region → allocation site (real allocations + prebound capture cells), as
    // in the group/activation walks — the structural key for the born-inside
    // and dies-inside gates.
    let mut region_alloc_hir: FxHashMap<Region, HirId> = FxHashMap::default();
    for (&hid, &reg) in &info.alloc_region {
        region_alloc_hir.insert(reg, hid);
    }
    for (&begin_id, cells) in &info.begin_cell_regions {
        for &(_b, reg) in cells {
            region_alloc_hir.insert(reg, begin_id);
        }
    }

    // The fiber-frontier-only seed halves (emit / send): the returned root may
    // cross the RETURN frontier — that is the shape — but an emitted/sent root
    // has an unbounded second consumer and refuses.
    let mut fiber_seeds: FxHashSet<Region> = FxHashSet::default();
    for (&b, regions) in &info.binding_source_regions {
        if escape.escapes_fiber(b) {
            fiber_seeds.extend(regions.iter().copied());
        }
    }
    for (&hid, &r) in &info.alloc_region {
        if escape.escapes_fiber_frontier(hid) {
            fiber_seeds.insert(r);
        }
    }

    // Regions already claimed by the merge forest (builder-idiom or
    // closure-cycle): never transfer members (the one-owner invariant).
    let is_merged = |r: Region| -> bool {
        info.merged_parent.contains_key(&r)
            || info.merged_parent.values().any(|&p| p == r)
            || info.closure_cycle_members.contains(&r)
    };
    // A region touched by any edge of any kind, in either role — the consumer
    // gate's "appears in no edge" test (hard may-stores included: a may-holder
    // may hold).
    let in_any_edge = |r: Region| -> bool {
        info.cross_region_refs
            .iter()
            .any(|&(_, s, d)| s == r || d == r)
            || capture_edges.iter().any(|&(_, s, d)| s == r || d == r)
            || info
                .containment_edges
                .iter()
                .any(|&(_, s, d)| s == r || d == r)
    };

    // ── The producer summary ────────────────────────────────────────────────
    // The returned subtree of lambda `l`, or `None` when any gate refuses.
    let summarize = |l: &Hir| -> Option<Summary> {
        let HirKind::Lambda { body, .. } = &l.kind else {
            return None;
        };
        let l_low = low.get(&l.id).copied().unwrap_or(0);
        let l_hi = ord(l.id);
        let inside = |id: HirId| -> bool {
            let o = ord(id);
            l_low <= o && o <= l_hi
        };
        // The tail must resolve, through the structural wrappers, to a single
        // binding read with exactly one source region — the root. A branch
        // mix, a bare call, or an aggregate tail refuses (no single region to
        // hand to the consumer's value-resolved adopt).
        fn tail_root(h: &Hir, info: &RegionInfo) -> Option<Region> {
            match &h.kind {
                HirKind::Return { value } => tail_root(value, info),
                HirKind::Begin(es) => tail_root(es.last()?, info),
                HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => tail_root(body, info),
                HirKind::DerefCell { cell } => tail_root(cell, info),
                HirKind::Var(b) => match info.binding_source_regions.get(b)?.as_slice() {
                    [r] => Some(*r),
                    _ => None,
                },
                _ => None,
            }
        }
        let root = tail_root(body, info)?;
        // The root is born in the producer, is fresh (a Fresh call-result or a
        // live local allocation — an opaque result could be a borrow), crosses
        // no fiber frontier, and carries no dynamic-lifetime class. It DOES
        // cross the return frontier — that is the transfer.
        if !inside(region_alloc_hir.get(&root).copied()?) {
            return None;
        }
        let root_fresh = info.fresh_result_regions.contains(&root)
            || (info.live_regions.contains(&root) && !info.call_result_regions.contains(&root));
        if !root_fresh
            || fiber_seeds.contains(&root)
            || info.cell_release_regions.contains(&root)
            || info.suppressed_decref_regions.contains(&root)
            || info.mutated_binding_value_regions.contains(&root)
            || is_merged(root)
        {
            return None;
        }
        // Members: born and last-used inside the producer, no frontier, no
        // dynamic class, sole-held, unclaimed.
        let subtree = inputs.reach(root);
        for &m in &subtree {
            if m == root {
                continue;
            }
            if inputs.not_ownable(info, m) || !inputs.sole_held(m) || is_merged(m) {
                return None;
            }
            let &alloc = region_alloc_hir.get(&m)?;
            let dp = info.region_data.get(&m).map(|d| d.decref_point)?;
            if !inside(alloc) || !inside(dp) {
                return None;
            }
        }
        // External uniqueness: nothing inside references out (the return
        // itself records no edge).
        if inputs.outside_ref_in(info, &subtree) {
            return None;
        }
        // The subtree must contain an interior cycle: an acyclic returned
        // subtree reclaims promptly by the RC cascade today, and adopting it
        // would only trade that promptness for the activation bound.
        let has_cycle = subtree.iter().any(|&m| {
            inputs
                .reach(m)
                .iter()
                .any(|&m2| m2 != m && inputs.reach(m2).contains(&m))
        });
        if !has_cycle {
            return None;
        }
        // Interior owner edges, exactly as the store/capture adopt assigns
        // them — plus the funnel store face: a containment edge is emittable
        // at the funnel call site recording the stored member (the adopt is
        // value-resolved there and needs no store opcode).
        let interior_store: Vec<(HirId, Region, Region)> = info
            .cross_region_refs
            .iter()
            .copied()
            .filter(|(site, s, d)| {
                !info.hard_edge_sites.contains(site) && subtree.contains(s) && subtree.contains(d)
            })
            .collect();
        let interior_capture: Vec<(HirId, Region, Region)> = capture_edges
            .iter()
            .copied()
            .filter(|(_l, s, d)| subtree.contains(s) && subtree.contains(d))
            .collect();
        // The emit site is the containment edge's own funnel call, inside the
        // producer — gated to a retaining-store site recording `s` (the stored
        // value; a `%del`/key read never retains, so it stays unemittable).
        let interior_funnel: Vec<(HirId, Region, Region)> = info
            .containment_edges
            .iter()
            .copied()
            .filter(|&(site, s, d)| {
                subtree.contains(&s)
                    && subtree.contains(&d)
                    && inside(site)
                    && info
                        .funnel_store_sites
                        .get(&site)
                        .is_some_and(|stored| stored.contains(&s))
            })
            .collect();
        // Single owner per member: the root when a direct edge exists, else
        // the unique interior container; the chosen edge must be emittable.
        let mut containers_of: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
        for &(_, s, d) in interior_store
            .iter()
            .chain(interior_capture.iter())
            .chain(interior_funnel.iter())
        {
            containers_of.entry(s).or_default().insert(d);
        }
        let mut store_edges: Vec<(HirId, Region, Region)> = Vec::new();
        let mut cap_edges: Vec<(HirId, Region, Region)> = Vec::new();
        for &m in &subtree {
            if m == root {
                continue;
            }
            let owner = match containers_of.get(&m) {
                Some(cs) if cs.contains(&root) => root,
                Some(cs) if cs.len() == 1 => *cs.iter().next().unwrap(),
                _ => return None,
            };
            // Prefer a store/funnel site (the edge's own site), else the
            // capture's closure-construction site.
            if let Some(&(site, s, d)) = interior_store
                .iter()
                .chain(interior_funnel.iter())
                .find(|&&(_, s, d)| s == m && d == owner)
            {
                store_edges.push((site, s, d));
            } else if let Some(&(lambda, s, d)) = interior_capture
                .iter()
                .find(|&&(_, s, d)| s == m && d == owner)
            {
                cap_edges.push((lambda, s, d));
            } else {
                return None;
            }
        }
        Some(Summary {
            root,
            store_edges,
            capture_edges: cap_edges,
        })
    };

    // ── The consumer gate ───────────────────────────────────────────────────
    // A call site's result region is adoptable iff it crosses no frontier,
    // appears in no edge, belongs to no dynamic class, and is discard-shaped:
    // every binding the result flows into is read only through the
    // Immediate-native allowance (extraction through a pass-through native
    // records no edge, so the shape gate is what keeps an uncounted member
    // borrow from escaping the node's reclamation horizon). The result flow is
    // followed BOTH through the site's own placeholder region and through the
    // producer's ROOT region: an inlined producer's result flows into the
    // caller's bindings as the root region directly (the walk's
    // `try_inline_call` returns the body's tail regions). A holder whose every
    // recorded init sits inside the producer is the producer's own binding
    // (its interior reads precede the return); a holder bound outside is a
    // consumer-side read and must pass the allowance.
    let consumer_result = |site: HirId, root: Region, l_low: u32, l_hi: u32| -> Option<Region> {
        let &r = info.alloc_region.get(&site)?;
        if shared.contains(&r)
            || in_any_edge(r)
            || info.cell_release_regions.contains(&r)
            || info.suppressed_decref_regions.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || is_merged(r)
        {
            return None;
        }
        let reads_pass = |b: Binding| -> bool {
            ix.uses
                .get(&b)
                .is_none_or(|uses| uses.iter().all(|u| matches!(u, UseForm::ImmediateArg)))
        };
        // The bindings bound directly (or through the ANF wrapper) to this
        // call's value.
        if let Some(holders) = ix.bound_to.get(&site) {
            for &b in holders {
                if !reads_pass(b) {
                    return None;
                }
            }
        }
        // Every binding the result-flow regions reach, outside the producer.
        for (b, regions) in &info.binding_source_regions {
            if !(regions.contains(&r) || regions.contains(&root)) {
                continue;
            }
            // Producer-internal holder: every recorded init sits inside the
            // producer's own subtree interval (its reads precede the return).
            let inside_producer = ix.inits.get(b).is_some_and(|inits| {
                inits.iter().all(|&i| {
                    // SAFETY: init pointers index the HIR tree, which outlives
                    // this call.
                    let o = ord(unsafe { &*i }.id);
                    l_low <= o && o <= l_hi
                })
            });
            if inside_producer {
                continue;
            }
            if !reads_pass(*b) {
                return None;
            }
        }
        Some(r)
    };

    let mut admitted: Vec<(Summary, Vec<Region>)> = Vec::new();
    let mut taken_roots: FxHashSet<Region> = FxHashSet::default();

    // ── The call face ───────────────────────────────────────────────────────
    // Producer candidates in structural walk order: bindings with exactly one
    // Lambda init, used ONLY as a callee, every call site consumer-admitted.
    {
        let mut candidates: Vec<(Binding, *const Hir)> = Vec::new();
        fn collect(
            h: &Hir,
            arena: &BindingArena,
            ix: &UseIndex,
            out: &mut Vec<(Binding, *const Hir)>,
        ) {
            match &h.kind {
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (b, init) in bindings {
                        if let Some(l) = ix.sole_lambda_init(*b, arena) {
                            if std::ptr::eq(l, unwrap_cell(init)) {
                                out.push((*b, l as *const Hir));
                            }
                        }
                        collect(init, arena, ix, out);
                    }
                    collect(body, arena, ix, out);
                }
                HirKind::Define { binding, value } => {
                    if let Some(l) = ix.sole_lambda_init(*binding, arena) {
                        if std::ptr::eq(l, unwrap_cell(value)) {
                            out.push((*binding, l as *const Hir));
                        }
                    }
                    collect(value, arena, ix, out);
                }
                _ => h.for_each_child(|c| collect(c, arena, ix, out)),
            }
        }
        collect(hir, arena, &ix, &mut candidates);

        for (f, l) in candidates {
            // SAFETY: `l` points into the HIR tree, which outlives this call.
            let l = unsafe { &*l };
            // Every occurrence of `f` must be a callee position — any other
            // use (an alias, a HOF hand-off, a store) is an unknown consumer.
            let Some(uses) = ix.uses.get(&f) else {
                continue;
            };
            let sites: Vec<HirId> = uses
                .iter()
                .filter_map(|u| match u {
                    UseForm::Callee(site) => Some(*site),
                    _ => None,
                })
                .collect();
            if sites.is_empty() || sites.len() != uses.len() {
                continue;
            }
            let Some(summary) = summarize(l) else {
                continue;
            };
            if taken_roots.contains(&summary.root) {
                continue;
            }
            let (l_low, l_hi) = (low.get(&l.id).copied().unwrap_or(0), ord(l.id));
            let Some(results) = sites
                .iter()
                .map(|&s| consumer_result(s, summary.root, l_low, l_hi))
                .collect::<Option<Vec<Region>>>()
            else {
                continue;
            };
            taken_roots.insert(summary.root);
            admitted.push((summary, results));
        }
    }

    // ── The fiber face ──────────────────────────────────────────────────────
    // A fiber whose body's terminal value is a summarized subtree: every
    // completing resume hands it back; every other outcome is a fresh error
    // struct or an immediate, each safely adoptable (and a re-delivered
    // masked-error payload is absorbed by the channel's idempotence, bounded
    // to one activation by the same-function gate).
    {
        // (fiber binding, fiber/new site, body lambda, the body's own binding
        // when it came through a Var), in walk order.
        type FiberCand = (Binding, HirId, *const Hir, Option<Binding>);
        fn collect_fibers(
            h: &Hir,
            ix: &UseIndex,
            arena: &BindingArena,
            cc: &CallClassification,
            out: &mut Vec<FiberCand>,
        ) {
            if let HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } = &h.kind {
                for (b, init) in bindings {
                    let call = anf_tail(init);
                    if let HirKind::Call { func, args, .. } = &call.kind {
                        let sym = callee_symbol(func, arena);
                        if sym.is_some() && sym == cc.fiber_new {
                            let body_lambda = args.first().and_then(|a| {
                                match &unwrap_cell(anf_tail(&a.expr)).kind {
                                    HirKind::Lambda { .. } => {
                                        Some((unwrap_cell(anf_tail(&a.expr)) as *const Hir, None))
                                    }
                                    HirKind::Var(fb) => ix
                                        .sole_lambda_init(*fb, arena)
                                        .map(|l| (l as *const Hir, Some(*fb))),
                                    _ => None,
                                }
                            });
                            if let Some((l, fb)) = body_lambda {
                                out.push((*b, call.id, l, fb));
                            }
                        }
                    }
                    collect_fibers(init, ix, arena, cc, out);
                }
                collect_fibers(body, ix, arena, cc, out);
                return;
            }
            h.for_each_child(|c| collect_fibers(c, ix, arena, cc, out));
        }
        let mut fibers: Vec<FiberCand> = Vec::new();
        collect_fibers(hir, &ix, arena, call_class, &mut fibers);

        for (f2, new_site, l, body_binding) in fibers {
            // SAFETY: `l` points into the HIR tree, which outlives this call.
            let l = unsafe { &*l };
            let bi = arena.get(f2);
            if !bi.is_immutable || bi.is_mutated || captured.contains(&f2) {
                continue;
            }
            // The body must be unable to deliver a non-terminal value: no
            // yield / io / debug / wait bits and not polymorphic. Errors are
            // fine — an error delivery is a fresh struct, and a restarted
            // re-delivery of the same payload lands in the same activation,
            // where the channel's idempotence absorbs it.
            let HirKind::Lambda {
                inferred_signals, ..
            } = &l.kind
            else {
                continue;
            };
            let suspending = crate::signals::SIG_YIELD
                .union(crate::signals::SIG_IO)
                .union(crate::signals::SIG_DEBUG)
                .union(crate::signals::SIG_WAIT);
            if inferred_signals.propagates != 0 || inferred_signals.bits.intersects(suspending) {
                continue;
            }
            // When the body lambda came through a binding, that binding's
            // every use must be a fiber-body position — a body ALSO called
            // directly (or handed anywhere else) has un-gated consumers.
            if let Some(fb) = body_binding {
                let all_fiber_arg = ix
                    .uses
                    .get(&fb)
                    .is_some_and(|us| us.iter().all(|u| matches!(u, UseForm::FiberNewArg0(_))));
                if !all_fiber_arg {
                    continue;
                }
            }
            // The fiber's own region: fresh, unaliased, edge-free, held only
            // by this binding.
            let Some(&rf2) = info.alloc_region.get(&new_site) else {
                continue;
            };
            if shared.contains(&rf2) || in_any_edge(rf2) || !inputs.sole_held(rf2) {
                continue;
            }
            if info.binding_source_regions.get(&f2).map(|v| v.as_slice()) != Some([rf2].as_slice())
            {
                continue;
            }
            // Every use of f2 is a resume (a gated consumer site) or an
            // Immediate-native read, all in the SAME function body as the
            // binding — each activation then drives its own private fiber, so
            // no delivery can outlive the adopting activation.
            let Some(uses) = ix.uses.get(&f2) else {
                continue;
            };
            let new_encl = ix.enclosing.get(&new_site).copied().unwrap_or(None);
            let mut resume_sites: Vec<HirId> = Vec::new();
            let mut ok = true;
            for u in uses {
                match u {
                    UseForm::ResumeArg0(site) => {
                        if ix.enclosing.get(site).copied().unwrap_or(None) != new_encl {
                            ok = false;
                            break;
                        }
                        resume_sites.push(*site);
                    }
                    UseForm::ImmediateArg => {}
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok || resume_sites.is_empty() {
                continue;
            }
            let Some(summary) = summarize(l) else {
                continue;
            };
            if taken_roots.contains(&summary.root) {
                continue;
            }
            let (l_low, l_hi) = (low.get(&l.id).copied().unwrap_or(0), ord(l.id));
            let Some(results) = resume_sites
                .iter()
                .map(|&s| consumer_result(s, summary.root, l_low, l_hi))
                .collect::<Option<Vec<Region>>>()
            else {
                continue;
            };
            taken_roots.insert(summary.root);
            admitted.push((summary, results));
        }
    }

    for (summary, results) in admitted {
        for (site, m, owner) in summary.store_edges {
            out.store.entry(site).or_default().push((m, owner));
        }
        for (site, m, owner) in summary.capture_edges {
            out.capture.entry(site).or_default().push((m, owner));
        }
        out.result_regions.extend(results);
    }
    out
}
