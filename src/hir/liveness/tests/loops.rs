use super::*;

#[test]
fn last_use_binding_used_in_loop_body_extends_to_loop() {
    // (let [s (string "a")] (while true (f s)))
    //
    // Macro-expansion of `(while c body)` introduces a Loop wrapping
    // the body, so the structurally relevant scope is the Loop node.
    // `s` is bound by the outer let. The body uses `s`. The
    // `s`-bound value (the string alloc) must survive the loop,
    // not die at the inner (f s) Call inside the body.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (let [s (string \"a\")] (while true (f s))))");
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected exactly one (string ...) alloc");
    let alloc = allocs[0];
    let loop_id = find_first_loop(&hir).expect("expected a Loop node");

    let got = info
        .last_use
        .get(&alloc)
        .copied()
        .expect("missing last_use");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(got) >= ord(loop_id),
        "(string \"a\") alloc @{} bound to a let-binding used inside loop @{} \
             must have last_use at or after the loop in execution order so the \
             region survives across iterations; got last_use=@{}",
        alloc.0,
        loop_id.0,
        got.0,
    );
}

// ── over-extension: bindings bound INSIDE the loop body must NOT
// extend to the loop's HirId ────────────────────────────────────
//
// Companion to last_use_binding_used_in_loop_body_extends_to_loop.
// A Var's last_use is extended to the outermost iter_scope's HirId
// only for bindings bound OUTSIDE the loop. For a binding bound INSIDE
// the loop body such an extension would be unsound: the lowerer would
// emit a DecrefRegion for the binding's region at the extended
// decref_point — outside the loop body — but the bytecode alloc lives
// inside the loop body. When the iterator is empty (or no iteration
// produces the alloc), the alloc never fires but the decref still does,
// hitting the phantom-region debug_assert in
// `RegionStore::decref_with_cascade`.
//
// Minimal repro: `(each x in @[] (let [f (fn () 1)] f))`. The each
// macro lowers to a while; `f`'s init `(fn () 1)` is INSIDE the
// while body. With the over-extension, `f`'s region's decref_point
// lands at the while's HirId — outside the body. Empty input → no
// MakeClosure → DecrefRegion fires on a never-allocated slot.
#[test]
fn last_use_var_to_binding_bound_inside_loop_does_not_extend() {
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (let [seq @[1]] (while (%lt 0 1) (let [f (fn () 1)] f))))");
    let loop_id = find_first_loop(&hir).expect("expected a Loop node");
    // Find the `f` Var — the binding whose name resolves to "f".
    let var_id = {
        fn find_var_named(
            h: &super::Hir,
            arena: &BindingArena,
            symbols: &SymbolTable,
            name: &str,
        ) -> Option<HirId> {
            if let HirKind::Var(b) = &h.kind {
                if symbols.name(arena.get(*b).name) == Some(name) {
                    return Some(h.id);
                }
            }
            let mut found = None;
            h.for_each_child(|c| {
                if found.is_none() {
                    found = find_var_named(c, arena, symbols, name);
                }
            });
            found
        }
        find_var_named(&hir, &arena, &symbols, "f").expect("expected a Var(f)")
    };
    let got = info
        .last_use
        .get(&var_id)
        .copied()
        .expect("missing last_use for Var inside loop");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(got) < ord(loop_id),
        "Var @{} (name=f) references a binding bound INSIDE the loop body @{}; \
             its last_use must NOT be extended to the loop (got last_use=@{}, \
             which is at or after the loop in execution order). Over-extension \
             here makes the lowerer emit DecrefRegion outside the loop body, \
             panicking on the phantom-region debug_assert when the loop never \
             executes (e.g. empty iterator).",
        var_id.0,
        loop_id.0,
        got.0,
    );
}

// Counterpart to the previous test: a binding bound OUTSIDE the loop
// by a PRECEDING SIBLING (a `def` earlier in the same body, not an
// enclosing `let`) and referenced inside the loop MUST have its
// last_use extended to the loop — its value is re-read every
// iteration, so freeing it after the first use dangles it (the
// minimized supervisor.lisp UAF, `loop-def-closure-uaf.lisp`).
//
// The `def` node is a sibling that precedes the loop, so its
// post-order index is SMALLER than the loop's — a plain
// `order[scope] > order[loop]` test (which only recognises an
// enclosing ancestor) would classify it as bound-inside and NOT
// extend. The interval test (`low[loop] <= order[scope]`) sees the
// `def` sits below the loop's subtree and extends correctly.
#[test]
fn last_use_var_to_def_bound_before_loop_extends_to_loop() {
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn () (def helper (fn (x) x)) (while (%lt 0 1) (helper 1)))");
    let loop_id = find_first_loop(&hir).expect("expected a Loop node");
    let var_id = {
        fn find_var_named(
            h: &super::Hir,
            arena: &BindingArena,
            symbols: &SymbolTable,
            name: &str,
        ) -> Option<HirId> {
            if let HirKind::Var(b) = &h.kind {
                if symbols.name(arena.get(*b).name) == Some(name) {
                    return Some(h.id);
                }
            }
            let mut found = None;
            h.for_each_child(|c| {
                if found.is_none() {
                    found = find_var_named(c, arena, symbols, name);
                }
            });
            found
        }
        find_var_named(&hir, &arena, &symbols, "helper")
            .expect("expected a Var(helper) inside the loop")
    };
    let got = info
        .last_use
        .get(&var_id)
        .copied()
        .expect("missing last_use for Var inside loop");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(got) >= ord(loop_id),
        "Var @{} (name=helper) references `helper`, bound by a `def` that \
             PRECEDES the loop @{} in the same body; its last_use must be \
             extended to the loop so the binding survives every iteration \
             (got last_use=@{}, which is BEFORE the loop in execution order — \
             the lowerer would free it after the first iteration, dangling \
             the closure: the supervisor.lisp use-after-free).",
        var_id.0,
        loop_id.0,
        got.0,
    );
}

// Nested loops: a binding bound BETWEEN two loops — INSIDE the outer
// loop body but OUTSIDE the inner loop — and referenced inside the inner
// loop must outlive the INNER loop (its body re-reads it every inner
// iteration). It need NOT outlive the OUTER loop: it is re-bound each
// outer iteration, so its region must be freed within the outer body.
//
// The extension targets the OUTERMOST loop the binding is bound OUTSIDE
// of — here the inner loop, not the absolute-outermost. Consulting only
// the absolute-outermost iter-scope (`iter_scope_stack.first()`) would be
// wrong: such a binding is bound INSIDE the outermost loop, so
// `bound_outside` is false there, and its last_use would not be extended —
// the decref_point would stay inside the inner loop body and the lowerer
// would free the value (decref + nil-stamp) after the inner loop's FIRST
// iteration. The next inner read would see nil; for an indexed-sequence
// `each` the freed binding is the inner loop's own `len`, so the bound
// check `(%lt idx nil)` would raise `%lt: ... integer and nil`
// (tests/elle/portrait.lisp, tests/elle/nested-loop-inner-invariant.lisp).
// The two bounds below pin the behavior: at or after the inner loop
// (survives it) AND strictly before the outer loop (not over-extended —
// over-extension would leak the prior iterations' values and re-introduce
// the phantom-region hazard).
#[test]
fn last_use_binding_between_nested_loops_extends_to_inner_loop() {
    // Outer while; `s` def'd inside the outer body to a COMPUTED value (so
    // it owns a region); inner while reads `s`. `s`'s scope (the Define)
    // is inside the outer loop's subtree but outside the inner loop's.
    let (hir, arena, symbols, info) = analyze_with_hir(
        "(fn () (while (%lt 0 1) (def s (string \"a\")) (while (%lt 0 1) (f s))))",
    );
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(allocs.len(), 1, "expected exactly one (string ...) alloc");
    let alloc = allocs[0];

    // Collect every Loop node and rank by execution order. The outer loop
    // is an ancestor of the inner, so it ranks AFTER it (larger ord); thus
    // inner = min-ord, outer = max-ord.
    let loops = {
        let mut out = Vec::new();
        fn walk(h: &super::Hir, out: &mut Vec<HirId>) {
            if matches!(&h.kind, HirKind::Loop { .. }) {
                out.push(h.id);
            }
            h.for_each_child(|c| walk(c, out));
        }
        walk(&hir, &mut out);
        out
    };
    assert_eq!(
        loops.len(),
        2,
        "expected exactly two Loop nodes (two whiles)"
    );
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let inner_loop = *loops.iter().min_by_key(|id| ord(**id)).unwrap();
    let outer_loop = *loops.iter().max_by_key(|id| ord(**id)).unwrap();

    let got = info
        .last_use
        .get(&alloc)
        .copied()
        .expect("missing last_use for the (string ...) alloc");
    assert!(
        ord(got) >= ord(inner_loop),
        "(string \"a\") alloc @{} is bound (via `s`) between the outer loop \
             @{} and the inner loop @{}, and read inside the inner loop; its \
             last_use must be at or after the INNER loop so the value survives \
             every inner iteration (got last_use=@{}, inside the inner body — \
             the lowerer frees it after the inner loop's first iteration, the \
             nested-each `%lt: integer and nil` bug).",
        alloc.0,
        outer_loop.0,
        inner_loop.0,
        got.0,
    );
    assert!(
        ord(got) < ord(outer_loop),
        "(string \"a\") alloc @{} is re-bound each OUTER iteration, so its \
             last_use must be strictly before the outer loop @{} in execution \
             order — extending past it would leak every prior iteration's value \
             and force a DecrefRegion outside the binding's per-iteration scope \
             (got last_use=@{}).",
        alloc.0,
        outer_loop.0,
        got.0,
    );
}

// Direct guard on the property that makes the loop-extension logic
// robust: an execution-order index must rank by STRUCTURE, not by
// HirId magnitude. ANF appends synthetic `let` bindings with fresh,
// high HirIds even when they sit inside a loop body — so a binding
// bound INSIDE a loop can carry an id LARGER than the loop. Comparing
// HirId magnitude would misclassify such a binding as "bound outside",
// over-extending its region's decref_point to the loop and producing a
// phantom DecrefRegion on an empty iterator. A degenerate compute_order
// that returned HirId.0 would fail this.
