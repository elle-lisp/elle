use super::*;

/// Counter-factual for the Family E capture-cell UAF: a `(begin (def x
/// v) (defn f [] x) ...)` form pre-allocates a `MakeCaptureCell` for
/// `x` at the Begin's HirId, and a sibling top-level form re-uses the
/// same binding (here, the final `x` reference at the file's top level).
/// The cell's region must outlive every use of `x`, including the
/// sibling form's. Without this, the cell is freed at the Begin's
/// `decref_point` and the next top-level access reads through a dangling
/// `CaptureCell` Value into a region that's been reclaimed — canonical
/// UAF caught by `as_capture_cell` at `handle_update_capture` /
/// `handle_unwrap_capture`. Minimal in-file reproducer matches
/// `tests/elle/destructuring.lisp` forms 70 + 187.
#[test]
fn begin_capture_cell_region_extends_to_binding_last_use_across_sibling_forms() {
    use crate::symbol::SymbolTable;
    let mut symbols = SymbolTable::new();
    // Two top-level forms:
    //   1) `(begin (def x 100) (defn f [] x) (f))` — x is captured by f
    //      → MakeCaptureCell stamped at this Begin's HirId.
    //   2) `x` — a sibling top-level use of x, lexically after the begin.
    let source = "(begin (def x 100) (defn f [] x) (f)) x";
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);

    // x's binding.
    let x = find_binding_by_name(&hir, "x", &arena).expect("expected binding `x`");

    // Find the Begin node that pre-allocates a CaptureCell because it
    // contains a `(define x …)` whose binding `needs_capture()`.
    fn find_capture_pre_pass_begin(
        hir: &Hir,
        target: Binding,
        arena: &BindingArena,
    ) -> Option<HirId> {
        fn contains_capturable_define(h: &Hir, target: Binding, arena: &BindingArena) -> bool {
            match &h.kind {
                HirKind::Define { binding, .. } => {
                    *binding == target && arena.get(*binding).needs_capture()
                }
                HirKind::Lambda { .. } => false,
                _ => {
                    let mut found = false;
                    h.for_each_child(|c| {
                        if !found {
                            found = contains_capturable_define(c, target, arena);
                        }
                    });
                    found
                }
            }
        }
        fn walk(hir: &Hir, target: Binding, arena: &BindingArena, out: &mut Option<HirId>) {
            if out.is_some() {
                return;
            }
            if let HirKind::Begin(exprs) = &hir.kind {
                if exprs
                    .iter()
                    .any(|e| contains_capturable_define(e, target, arena))
                {
                    *out = Some(hir.id);
                    return;
                }
            }
            hir.for_each_child(|c| walk(c, target, arena, out));
        }
        let mut out = None;
        walk(hir, target, arena, &mut out);
        out
    }
    let begin_id = find_capture_pre_pass_begin(&hir, x, &arena)
        .expect("expected a Begin pre-allocating a CaptureCell for x");

    // The cell's region: minted per binding in the Begin walk
    // (`begin_cell_regions` — one region PER cell, never one shared
    // alloc_region for all of a Begin's cells).
    let cell_region = info
        .begin_cell_regions
        .get(&begin_id)
        .and_then(|cells| cells.iter().find(|(b, _)| *b == x).map(|&(_, r)| r))
        .expect("Begin with capturable Define must register x's cell in begin_cell_regions");

    // The post-pass extends region_data[r].decref_point via binding uses
    // when r ∈ binding_source_regions[x]. The invariant under test is
    // that the cell's region is covered, so its decref_point reaches x's
    // last use, not just the Begin's tail. Equivalent end-state
    // check: region_data[cell_region].decref_point >= last_use_of(x).
    let mut du = DefUseBuilder::new();
    du.walk(&hir);
    let order = crate::hir::liveness::compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let last_use = crate::hir::liveness::compute_last_use(&hir, &du.uses, &order).per_node;
    let x_last_use = du
        .uses
        .get(&x)
        .into_iter()
        .flat_map(|v| v.iter())
        .map(|u| last_use.get(u).copied().unwrap_or(*u))
        .max_by_key(|id| ord(*id))
        .expect("x must have at least one use (the final top-level `x`)");

    let cell_decref_point = info
        .region_data
        .get(&cell_region)
        .map(|d| d.decref_point)
        .expect("cell region must have RegionData populated by analyze_regions");

    // Ordering compared via the structural index, not HirId
    // magnitude (which ANF makes meaningless — see compute_order).
    assert!(
        ord(cell_decref_point) >= ord(x_last_use),
        "cell region r{} (CaptureCell for x, pre-allocated at Begin @{}) \
             must have decref_point >= x's last use @{}; got decref_point @{}",
        cell_region.0,
        begin_id.0,
        x_last_use.0,
        cell_decref_point.0,
    );
}

/// Counter-factual for the env-cell-in-loop UAF
/// (tests/elle/region-capture-cell-loop-uaf.lisp, cap2.lisp). A
/// `@`-mutable captured local DEFINED INSIDE a loop and captured by a closure
/// built in that loop is a `populate_env` env cell minted EXACTLY ONCE per
/// activation (the box is not re-allocated per iteration; only its content is
/// re-stored). Its `DecrefCellRegion` must therefore also fire once per
/// activation. If the binding-chain `decref_point` extension left it at the
/// binding's in-loop last use, it would fire per iteration: a closure called in
/// place and dying within the iteration nets the box region -1 each pass
/// (capture-incref +1, free-cascade -1, DecrefCellRegion -1), freeing the
/// once-allocated box on iteration 1 → the next iteration reads the recycled
/// cell. `hoist_cell_release_past_loops` lifts a cell-release region's
/// decref_point to the outermost enclosing While/Loop, which the lowerer emits
/// AFTER the loop. Assert the hoist at the region-analysis layer:
/// region_data[cell_region].decref_point is at/after the enclosing While node,
/// never strictly inside its body.
#[test]
fn env_cell_release_in_loop_hoisted_past_loop() {
    use crate::symbol::SymbolTable;
    // A function whose body loops, defining a `@`-mutable `s` inside the loop
    // and capturing it with a closure called in place — the minimal repro shape.
    // Compiled through the real file pipeline so `while` lowers to HirKind::While
    // exactly as the elle reproducer does.
    // Only intrinsics/literals/callable-array — `compile_file_to_fhir` loads no
    // prelude, so `+`/`get` are undefined. The closure reads an ELEMENT via the
    // callable-array form `(s 0)`, NOT the cell itself: returning the cell would
    // put its region in the return frontier (escape's return facet), where the
    // ownership analysis would treat it as the closure's tail value and elide the
    // per-iteration decref, masking the bug. Reading an element
    // keeps the env cell's release live and in-loop — exactly the faulting
    // shape (verified: this source UAFs under the plain VM before the fix).
    let source = "(defn go [] \
                     (def @acc 0) (def @i 0) \
                     (while (%lt i 3) \
                       (def @s @[10]) \
                       (let [cl (fn [] (s 0))] (assign acc (cl))) \
                       (assign i (%add i 1))) \
                     acc)";
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);

    // The env-cell release region for `s`: `env_cell_placeholder` (Define arm)
    // records it in binding_source_regions[s] AND in cell_release_regions. Find
    // the (binding, region) pair directly by the binding's name, which is robust
    // to how the capture-read is wrapped in the lowered HIR.
    let (s, cell_region) = info
        .binding_source_regions
        .iter()
        .find_map(|(b, regions)| {
            if symbols.name(arena.get(*b).name) != Some("s") {
                return None;
            }
            let cr = regions
                .iter()
                .copied()
                .find(|r| info.cell_release_regions.contains(r))?;
            Some((*b, cr))
        })
        .expect("`s` must have an env-cell release region (captured @-mutable local in a lambda)");
    let _ = s;

    // Every iter-scope node (While OR Loop — `while` may lower to either) and
    // its post-order subtree interval [low, order].
    fn find_iter_scopes(hir: &Hir) -> Vec<HirId> {
        let mut out = Vec::new();
        fn walk(hir: &Hir, out: &mut Vec<HirId>) {
            if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
                out.push(hir.id);
            }
            hir.for_each_child(|c| walk(c, out));
        }
        walk(hir, &mut out);
        out
    }
    let order = crate::hir::liveness::compute_order(&hir);
    let low = crate::hir::liveness::compute_subtree_low(&hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let whiles = find_iter_scopes(&hir);
    assert!(
        !whiles.is_empty(),
        "repro must contain an iter-scope (While/Loop)"
    );

    let cell_decref = info
        .region_data
        .get(&cell_region)
        .map(|d| d.decref_point)
        .expect("cell region must have RegionData populated by analyze_regions");

    // The box is minted once per activation (populate_env), so its
    // DecrefCellRegion must fire once per activation — at/after the enclosing
    // loop, NEVER strictly inside a loop body (a proper descendant of a While).
    // Hoisted, decref_point IS the loop node (the post-loop emission point), so
    // it is allowed to equal a While; only a strict-descendant placement is the
    // per-iteration bug. Compared via the structural order index, not HirId
    // magnitude (which ANF makes meaningless — see compute_order).
    let dord = ord(cell_decref);
    let strictly_inside = whiles.iter().find(|&&w| {
        let lo = low.get(&w).copied().unwrap_or(0);
        dord >= lo && dord <= ord(w) && cell_decref != w
    });
    assert!(
        strictly_inside.is_none(),
        "env-cell release for `s` (r{}) must be hoisted to/after the enclosing \
         loop (once per activation), not left strictly inside the body of While \
         @{}; got decref_point @{}",
        cell_region.0,
        strictly_inside.map(|w| w.0).unwrap_or(0),
        cell_decref.0,
    );
}

/// Counter-factual for the unrecorded-binder hoist
/// (tests/elle/region-match-bind-loop.lisp). A `match` arm's pattern binds a
/// projection out of the scrutinee by an UNCOUNTED read, so the projection
/// resolves to the scrutinee's own region and the binding-chain extension carries
/// the scrutinee's release out to wherever the projection is last used. Inside a
/// loop, that read is put through the iter-scope extension, which asks whether the
/// binding is bound OUTSIDE the loop — a containment test over the binding's
/// recorded scope node. A pattern that records no scope has none, absence reads as
/// bound-outside, and the release is hoisted to the loop node: one release for N
/// per-iteration scrutinees, N−1 held to fiber teardown
/// (docs/impl/region/mechanism.md § "Every binder records its scope").
///
/// Region-analysis invariant: the scrutinee's region is allocated in the loop
/// body, so its `decref_point` must stay STRICTLY inside the loop's subtree —
/// landing on the loop node itself is the hoist this pins against.
#[test]
fn match_pattern_binding_keeps_scrutinee_release_inside_the_loop() {
    use crate::hir::expr::IntrinsicOp;
    use crate::symbol::SymbolTable;

    // No-prelude file pipeline (intrinsics + special forms only). The scrutinee is
    // a fresh `%pair` per iteration and the taken arm READS the name its pattern
    // bound — the read is what places the scrutinee's release, so an arm that
    // ignored its binding would not exercise this at all.
    let source = "(defn go [] \
                     (def @i 0) \
                     (while (%lt i 3) \
                       (match (%pair i i) (x . y) x _ 0) \
                       (assign i (%add i 1))) \
                     i)";
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);

    // Every iter-scope node (While OR Loop — `while` may lower to either).
    fn find_iter_scopes(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
            out.push(hir.id);
        }
        hir.for_each_child(|c| find_iter_scopes(c, out));
    }
    let mut loops = Vec::new();
    find_iter_scopes(&hir, &mut loops);
    assert!(
        !loops.is_empty(),
        "repro must contain an iter-scope (While/Loop)"
    );

    // The scrutinee: the `%pair` the loop body allocates each iteration.
    fn find_pair(hir: &Hir, out: &mut Option<HirId>) {
        if out.is_none() {
            if let HirKind::Intrinsic {
                op: IntrinsicOp::Pair,
                ..
            } = &hir.kind
            {
                *out = Some(hir.id);
                return;
            }
            hir.for_each_child(|c| find_pair(c, out));
        }
    }
    let mut pair_id = None;
    find_pair(&hir, &mut pair_id);
    let pair_id = pair_id.expect("the loop body must allocate a %pair scrutinee");
    let scrutinee_region = info
        .alloc_region
        .get(&pair_id)
        .copied()
        .expect("the %pair scrutinee must have an alloc_region");

    let order = crate::hir::liveness::compute_order(&hir);
    let low = crate::hir::liveness::compute_subtree_low(&hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let decref = info
        .region_data
        .get(&scrutinee_region)
        .map(|d| d.decref_point)
        .expect("the scrutinee's region must have RegionData populated");
    let dord = ord(decref);

    // The allocation is per iteration, so the release must be too: strictly
    // inside the loop's subtree, never AT the loop node (the post-loop emission
    // point, where one release would cover every iteration) nor past it.
    let enclosing = loops.iter().copied().find(|&l| {
        let lo = low.get(&l).copied().unwrap_or(0);
        let alloc = ord(pair_id);
        alloc >= lo && alloc <= ord(l)
    });
    let enclosing = enclosing.expect("the %pair must sit inside a loop body");
    let lo = low.get(&enclosing).copied().unwrap_or(0);
    assert!(
        dord >= lo && dord < ord(enclosing),
        "the scrutinee's region r{} (allocated by the %pair @{} inside the loop \
         @{}) must be released per iteration — its decref_point must lie strictly \
         inside the loop's subtree [{}, {}), got @{} (index {})",
        scrutinee_region.0,
        pair_id.0,
        enclosing.0,
        lo,
        ord(enclosing),
        decref.0,
        dord,
    );
}

/// Counter-factual for the loop-local-closure tail UAF
/// (tests/elle/region-loop-local-closure-tail-uaf.lisp).
/// A closure created INSIDE a loop, called in place, whose body's tail is a
/// fresh allocation, is INLINABLE — so `try_inline_call` re-walks its body at
/// the call site (in the caller's discarding context) to discover edges. That
/// re-walk must NOT `alloc_here`-overwrite `alloc_region[body-node]` with a
/// fresh caller-context region: doing so desyncs it from the return-frontier
/// projection. The lowerer reads `alloc_region` and the ownership/compensation
/// passes read escape's return frontier *projected through* `alloc_region`, so a
/// clobbered entry makes the two disagree on which region the body's tail names —
/// a stale region spared by one path while the other emits a decref on the
/// clobbered one, so the closure frees the value it returns (the
/// stale-region-deref UAF).
///
/// Region-analysis invariant: the region the lowerer will emit for the closure
/// body's tail allocation (`alloc_region[tail]`) MUST be the region the return
/// frontier projects to for that tail. `alloc_here` is idempotent under an
/// inlined re-walk so the structural assignment is never clobbered.
#[test]
fn loop_local_closure_tail_alloc_region_matches_return_frontier() {
    use crate::hir::expr::IntrinsicOp;
    use crate::symbol::SymbolTable;

    // No-prelude file pipeline (intrinsics + special forms only). The closure
    // `(fn [] (%pair i i))` is bound by a `let` INSIDE the loop and called in
    // place — the inlinable, re-walked shape. `%pair` stays in tail position
    // (lowered as a tail call), so the lambda body's tail node IS the Pair
    // intrinsic.
    let source = "(defn go [] \
                     (def @i 0) \
                     (while (%lt i 3) \
                       (let [g (fn [] (%pair i i))] (g)) \
                       (assign i (%add i 1))) \
                     i)";
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);

    // Descend through tail-transparent wrappers (Let/Begin/Block/Return) to the
    // body's final expression — the value the lambda returns.
    fn tail_expr(h: &Hir) -> &Hir {
        match &h.kind {
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => tail_expr(body),
            HirKind::Begin(exprs) => exprs.last().map(tail_expr).unwrap_or(h),
            HirKind::Block { body, .. } => body.last().map(tail_expr).unwrap_or(h),
            HirKind::Return { value } => tail_expr(value),
            _ => h,
        }
    }
    // For each Lambda, record (lambda id, body-tail id, whether the tail is a
    // %pair allocation). Owned Copy data only — no escaping HIR references.
    fn collect_lambda_tails(h: &Hir, out: &mut Vec<(HirId, HirId, bool)>) {
        if let HirKind::Lambda { body, .. } = &h.kind {
            let t = tail_expr(body);
            let is_pair = matches!(
                &t.kind,
                HirKind::Intrinsic {
                    op: IntrinsicOp::Pair,
                    ..
                }
            );
            out.push((h.id, t.id, is_pair));
        }
        h.for_each_child(|c| collect_lambda_tails(c, out));
    }

    let mut lambda_tails_info = Vec::new();
    collect_lambda_tails(&hir, &mut lambda_tails_info);

    // The inner closure: its body's tail expression allocates a Pair.
    let (_lambda_id, tail_id, _) = lambda_tails_info
        .iter()
        .copied()
        .find(|&(_, _, is_pair)| is_pair)
        .expect("a closure whose body tail-allocates a %pair");

    let tail_region = info
        .alloc_region
        .get(&tail_id)
        .copied()
        .expect("the %pair tail must have an alloc_region the lowerer will emit");

    // The closure's body-tail %pair crosses the return frontier (it is the value the
    // closure hands back). That is escape's judgment — `record_frontier_sites`
    // records the atomless tail aggregate — projected to its region by
    // `region::infer::escape`. If it were absent the closure would free its own
    // tail-returned value (region-loop-local-closure-tail-uaf.lisp); the inlined
    // re-walk must therefore leave `alloc_region` in sync with the projection.
    let escape = crate::hir::analyze_escape(
        &hir,
        &arena,
        &crate::hir::region::CallClassification::default(),
    );
    let frontier = crate::hir::return_frontier_regions(
        &escape,
        &info.alloc_region,
        &info.binding_source_regions,
    );
    assert!(
        frontier.contains(&tail_region),
        "loop-local closure tail region r{} (alloc_region of the %pair body @{}, the \
         region the lowerer emits + releases) must be in the return frontier {:?} so the \
         escape analysis stays in sync with it (region-loop-local-closure-tail-uaf.lisp)",
        tail_region.0,
        tail_id.0,
        frontier.iter().map(|r| r.0).collect::<Vec<_>>(),
    );
}
