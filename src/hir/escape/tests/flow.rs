use super::*;

#[test]
fn empty_info_reports_no_escapes() {
    let info = EscapeInfo::empty();
    // Absence is the default: any binding or lambda not recorded as
    // escaping reads as non-escaping.
    assert!(!info.binding_escapes_activation(Binding(0)));
    assert!(!info.binding_escapes_activation(Binding(7)));
    assert!(!info.lambda_escapes_definition(HirId(0)));
    assert!(!info.lambda_escapes_definition(HirId(42)));
}

/// Interprocedural return: a tail call to an arg-returning callee propagates
/// the returned arg into the caller's tail, so the arg escapes its activation
/// **via return**. The arg-return summary (`compute_arg_return`) makes
/// `tail_sources` descend through such a call.
///
/// "Inlinable" is a `Let`/`Letrec`-bound, immutable, unmutated lambda — never a
/// top-level `Define` (the `def`-callee case, where the arg does NOT escape
/// through the call, is pinned in `def_callee_arg_does_not_escape_through_call`).
/// Each helper is called in place in its own defining scope, so none is
/// `is_captured` — isolating the return facet from the capture facet.
#[test]
fn interprocedural_return_escape() {
    // identity helper: the call returns arg 0, so `y` escapes f's tail via return.
    assert_binding_escape(
        "(def f (fn (y) (let [id (fn (x) x)] (id y)))) (f 1)",
        &[("y", true, true), ("x", true, true)],
    );
    // projection helper: only arg 0 (`a`) is returned and escapes; `b`, passed
    // but not returned, does NOT escape through the call. (`q`, an unused param,
    // has no `Var` reference to assert on.)
    assert_binding_escape(
        "(def f (fn (a b) (let [k (fn (p q) p)] (k a b)))) (f 1 2)",
        &[("a", true, true), ("b", false, false), ("p", true, true)],
    );
    // nested calls to the same uncaptured helper: `tail_sources` descends
    // interprocedurally through both.
    assert_binding_escape(
        "(def f (fn (y) (let [id (fn (x) x)] (id (id y))))) (f 1)",
        &[("y", true, true)],
    );
    // the call result is bound then returned: the binding-definition edge carries
    // the interprocedural source (`t` aliases the arg `y`).
    assert_binding_escape(
        "(def f (fn (y) (let [id (fn (x) x)] (let [t (id y)] t)))) (f 1)",
        &[("y", true, true), ("t", true, true)],
    );
}

/// Interprocedural return transparency is for `Let`/`Letrec`-bound lambdas only,
/// never a top-level `Define`: an arg returned *through a call to a `def`-bound fn*
/// does NOT reach the caller's tail. `compute_arg_return` excludes `def`-bound
/// callees, so the arg must not be marked return-escaping here. This pins the
/// `Define` exclusion as a hard boundary of the return facet.
#[test]
fn def_callee_arg_does_not_escape_through_call() {
    // `id` is a top-level def, not inlinable, so `y` does not escape via the
    // `(id y)` call. (Whether the inner `lambda` escapes is a separate facet; we
    // assert only about the argument binding here, by name.)
    let src = "(def id (fn (x) x)) (def f (fn (y) (id y))) (f 1)";
    let mut symbols = crate::symbol::SymbolTable::new();
    let (hir, arena, names) = compile_fhir(src, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let escape = analyze_escape(&hir, &arena, &pc.call_classification);

    let y = bindings_named(&hir, &arena, &["y"], &names);
    assert!(!y.is_empty(), "missing `y` in `{src}`");
    assert!(
        y.iter().all(|b| !escape.binding_escapes_activation(*b)),
        "`y` is wrongly marked escaping through a def-callee call (the solver \
             does not inline a top-level def, so this would over-mark)"
    );
}

/// White-box pin on `compute_arg_return`'s fixpoint: `g` returns its argument
/// only by tail-calling `id`, so `g`'s summary depends on `id`'s — the fixpoint
/// must converge to both `id → [0]` and `g → [0]`. `k` is a projection
/// (returns arg 1 only) and `c` returns a constant (no param returned). This
/// exercises the cross-binding chain directly.
///
/// Uses `let`, not `letrec`: `let`-bound names are immutable, so the
/// consumption guard (`tail_sources`'s `Call` arm) admits the descent. A `letrec`
/// binding is not immutable — the solver does
/// not inline it, and neither does this analysis — so the chain would not
/// thread through one (confirmed: a letrec callee produces no binding
/// under-mark in the solver).
#[test]
fn arg_return_summary_chains_through_the_fixpoint() {
    let src = "(let [id (fn (x) x) \
                         g (fn (z) (id z)) \
                         k (fn (a b) b) \
                         c (fn (w) 0)] \
                     (begin (id 1) (g 1) (k 1 2) (c 1)))";
    let mut symbols = crate::symbol::SymbolTable::new();
    let (hir, arena, names) = compile_fhir(src, &mut symbols);
    let summary = compute_arg_return(&hir, &arena);

    let lookup = |name: &str| -> Vec<usize> {
        let bs = bindings_named(&hir, &arena, &[name], &names);
        assert!(!bs.is_empty(), "missing `{name}` in `{src}`");
        summary.get(&bs[0]).cloned().unwrap_or_default()
    };
    assert_eq!(lookup("id"), vec![0], "id returns its only param");
    assert_eq!(
        lookup("g"),
        vec![0],
        "g returns arg 0 transitively via the id chain (the fixpoint)"
    );
    assert_eq!(lookup("k"), vec![1], "k is a projection onto arg 1");
    assert_eq!(
        lookup("c"),
        Vec::<usize>::new(),
        "c returns a constant — no param flows to its tail"
    );
}

/// The return facet (`binding_escapes_via_return`) is strictly narrower than
/// the full escape set (`binding_escapes_activation`) — the distinction the
/// reassign 1-slot-container gate depends on. A value that escapes only by
/// being STORED into a container, or CAPTURED by a closure, escapes its
/// activation but is NOT *returned*; only a value flowing to a tail/return is.
/// (Were the gate to use the full set, it would refuse the optimization for
/// container-stored and captured mutables — regressing `reassign_gate_keeps_*`.)
/// Also pins the structural invariant: returned ⟹ escapes.
#[test]
fn return_facet_is_narrower_than_full_escape() {
    let check = |src: &str, name: &str, want_full: bool, want_return: bool| {
        let mut symbols = crate::symbol::SymbolTable::new();
        let (hir, arena, names) = compile_fhir(src, &mut symbols);
        let meta = crate::primitives::build_primitive_meta(&mut symbols);
        let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
        let escape = analyze_escape(&hir, &arena, &pc.call_classification);
        let bs = bindings_named(&hir, &arena, &[name], &names);
        assert!(!bs.is_empty(), "missing `{name}` in `{src}`");
        for b in bs {
            assert_eq!(
                escape.binding_escapes_activation(b),
                want_full,
                "`{name}` full-escape mismatch in `{src}`"
            );
            assert_eq!(
                escape.binding_escapes_via_return(b),
                want_return,
                "`{name}` return-escape mismatch in `{src}`"
            );
            // Invariant: the return facet is a subset of the full set.
            assert!(
                !escape.binding_escapes_via_return(b) || escape.binding_escapes_activation(b),
                "`{name}` returned but not escaping — return facet must be a subset"
            );
        }
    };
    // Stored into the returned pair: escapes via STORE; `s` itself is not the
    // tail (the fresh pair is), so it is not returned.
    check(
        "(def f (fn (y) (let [s \"hi\"] (%pair s s)))) (f 1)",
        "s",
        true,
        false,
    );
    // Directly returned: escapes via RETURN.
    check("(def g (fn (y) (let [s \"hi\"] s))) (g 1)", "s", true, true);
    // Captured by a returned closure: `p` escapes via CAPTURE (the closure `c`
    // is returned and holds it), but `p` itself never flows to a tail.
    check(
        "(def h (fn () (let [p \"x\"] (let [c (fn () (length p))] c)))) (h)",
        "p",
        true,
        false,
    );
}

/// An `assign` / `set-cell!` in **tail position** returns the value it stores,
/// so the target cell escapes via return — even when the stored value is a
/// fresh, *atomless* result, the case a plain value-descent misses. `return_atoms`
/// seeds the target; without that seed the cell is under-marked, a reassign-gate
/// UAF risk (the gate would suppress the decref of a value that is in fact
/// returned).
///
/// Shape: a captured-cell setter `(fn (v) (assign c v))` — functionalize keeps
/// the assign as a `SetCell` at the closure's tail (the setter returns the
/// stored value), the one shape where a bare store survives at a tail (a
/// straight-line fn-local reassign is rewritten to a value-discarding let).
/// Asserts on the store TARGET directly (a `Binding` field / `Var` cell, not a
/// readable `Var`, so it is found by walking for `Assign`/`SetCell`).
#[test]
fn store_in_tail_position_marks_target_returned() {
    fn store_targets(h: &Hir, out: &mut Vec<Binding>) {
        match &h.kind {
            HirKind::Assign { target, .. } => out.push(*target),
            HirKind::SetCell { cell, .. } => {
                if let HirKind::Var(b) = &cell.kind {
                    out.push(*b);
                }
            }
            _ => {}
        }
        h.for_each_child(|c| store_targets(c, out));
    }
    let src = "(def make (fn (init) (def @c init) (fn (v) (assign c v)))) \
                   ((make 0) (%pair 1 2))";
    let (hir, arena, cc) = compile_with_cc(src);
    let escape = analyze_escape(&hir, &arena, &cc);
    let mut targets = Vec::new();
    store_targets(&hir, &mut targets);
    assert!(
        !targets.is_empty(),
        "no assign/set-cell at a tail in `{src}` — shape no longer exercises return_atoms"
    );
    for t in targets {
        assert!(
            escape.binding_escapes_via_return(t),
            "store target {t:?} at a tail must escape via return in `{src}`"
        );
    }
}

/// Curated intraprocedural return/tail shapes, asserted against escape's own
/// spec (`docs/impl/escape.md`). The return facet marks exactly the values that
/// flow to a tail; capture/store escapes are activation-escapes but not returns.
#[test]
fn return_escape_spec() {
    // identity: param escapes via return; the fn itself does not.
    assert_binding_escape("(def id (fn (x) x)) (id 5)", &[("x", true, true)]);
    // projection: only the returned param `a` escapes. (`b`, an unused param, has
    // no `Var` reference to assert on.)
    assert_binding_escape("(def k (fn (a b) a)) (k 1 2)", &[("a", true, true)]);
    // a returned closure `(fn () x)` whose body IS `x`: `x` flows to that closure's
    // tail, so it escapes via return (and via capture — the closure is returned).
    assert_binding_escape(
        "(def make (fn (x) (fn () x))) (make 1)",
        &[("x", true, true)],
    );
    // returned heap binding.
    assert_binding_escape("(let [s \"hello\"] s)", &[("s", true, true)]);
    // heap binding consumed locally, not returned.
    assert_binding_escape("(let [s \"hello\"] (length s))", &[("s", false, false)]);
    // a closure reached through a returned alias chain escapes via return.
    assert_binding_escape(
        "(letrec [f (fn (x) x)] (let [g f] g))",
        &[("f", true, true), ("g", true, true)],
    );
    // branch join: both arms' returned bindings escape.
    assert_binding_escape(
        "(def pick (fn (c a b) (if c a b))) (pick true 1 2)",
        &[("a", true, true), ("b", true, true), ("c", false, false)],
    );
    // begin tail: only the last expression flows out.
    assert_binding_escape("(def f (fn (x) (begin 99 x))) (f 7)", &[("x", true, true)]);
    // intra-fn alias: returning `y` escapes the `x` it was bound from.
    assert_binding_escape(
        "(def h (fn (x) (let [y x] y))) (h 4)",
        &[("x", true, true), ("y", true, true)],
    );
    // `c = (fn () x)` is called in place, but its body IS `x`, so `x` flows to `c`'s
    // tail and return-escapes regardless of whether `c` escapes. (The finer
    // capture-not-escape case — a closure that *uses* its upvalue without returning
    // it — is pinned by `lexical_capture_is_not_escape`.)
    assert_binding_escape(
        "(def consume (fn (x) (let [c (fn () x)] (c)))) (consume 1)",
        &[("x", true, true)],
    );
}

/// Store-into-a-longer-lived-region escape (the store facet). A value stored
/// into a freshly-allocated aggregate (`%pair`, an uncounted compile-time store)
/// escapes its activation but is NOT returned; the store *target* (the container)
/// is not a source and does not escape. A MUTABLE-container store (`%array-push`/
/// `%put`) is different: it compiles as a `Funnel` native call whose store is
/// runtime-counted (the funnel increfs the stored value; the container's
/// free-time cascade balances it), so there is no uncounted store for the caller
/// to account for and escape seeds NOTHING — neither the value nor the container.
#[test]
fn store_escape_spec() {
    // a let-local heap value stored into a pair: `s` escapes via the STORE, not
    // the return (only the pair's own region is the tail).
    assert_binding_escape("(let [s \"hi\"] (%pair s s))", &[("s", true, false)]);
    // two distinct let-locals, each stored into the pair.
    assert_binding_escape(
        "(let [s \"hi\" t \"yo\"] (%pair s t))",
        &[("s", true, false), ("t", true, false)],
    );
    // a closure stored into a pair escapes via the store (and so does its lambda,
    // by backward propagation — pinned at the binding here).
    assert_binding_escape("(let [f (fn () 1)] (%pair f f))", &[("f", true, false)]);
    // store reached through an alias chain: `t` is stored, and backward
    // propagation escapes the `s` it aliases.
    assert_binding_escape(
        "(let [s \"hi\"] (let [t s] (%pair t t)))",
        &[("s", true, false), ("t", true, false)],
    );
    // pushed value: the mutable-array store rides the runtime-counted funnel, so
    // neither the value nor the collection is an escape seed.
    assert_binding_escape(
        "(let [box @[] s \"hi\"] (%array-push box s))",
        &[("s", false, false), ("box", false, false)],
    );
    // put value: the mutable-struct store is the same funnel accounting.
    assert_binding_escape(
        "(let [m @{} s \"hi\"] (%put m :k s))",
        &[("s", false, false), ("m", false, false)],
    );
    // a value consumed by a non-storing op does NOT escape.
    assert_binding_escape("(let [s \"hi\"] (length s))", &[("s", false, false)]);
}

/// The store facet is keyed on the declared `RegionEffect`, so a native's
/// declaration is a claim about escape and not only about the may-store clique.
/// `Mixed`/`Unknown` seeds every argument here; the read-only trait dispatchers
/// declare `Opaque` and seed nothing, which is their whole cost at one heap
/// argument — the clique is over PAIRS of arguments and is empty either way
/// (docs/impl/region/effects.md § `Opaque`; docs/impl/escape.md).
#[test]
fn a_sequence_read_does_not_seed_the_store_facet() {
    for read in ["first", "second", "rest", "->array", "->list"] {
        // The read's result is consumed locally, so nothing pulls the container's
        // contents into a facet either — the subject is judged on the call alone.
        assert_binding_escape(
            &format!("(let [s (list 1 2)] (length ({read} s)))"),
            &[("s", false, false)],
        );
    }
    // The contrast: `git` is declared `Mixed` and genuinely so — it caches
    // compiled SPIR-V on its argument's template, a retention no seam records —
    // so its argument is seeded on the store facet.
    assert_binding_escape(
        "(let [s (fn (a b) a)] (length (git s)))",
        &[("s", true, false)],
    );
}

/// The fiber-graph natives are the same claim on a different subject, and the
/// WRITE side answers it the same way the read side does. `fiber/child` hands back
/// the cached child-fiber `Value` its argument carries; `fiber/propagate` hands its
/// argument to the `SIG_PROPAGATE` handler, which writes it into the propagating
/// fiber's `child`/`child_value` pair. That pair is not enumerated by the free-time
/// walk, so neither call creates a holder and neither argument is a store-facet
/// seed (docs/impl/region/effects.md § `Opaque`, "The child-chain WIRING is
/// `Opaque` too"). The counter-factual is `Mixed` on `fiber/propagate`, and this
/// seed is the only thing that catches it: `Mixed` reads true on the result side
/// (the SIG_PROPAGATE payload IS arg0) and the declaration oracle exempts a
/// signal-carrying return, so no result check can. What it costs is `defer`'s
/// success path, which names its fiber in the arm this call does not take.
#[test]
fn a_fiber_graph_write_does_not_seed_the_store_facet() {
    for op in ["fiber/child", "fiber/propagate"] {
        assert_binding_escape(
            &format!("(let [f (fiber/new (fn () 1) |:error|)] (fiber? ({op} f)))"),
            &[("f", false, false)],
        );
    }
}

/// `import` copies its specifier out to a Rust `String` to resolve it and stores
/// no argument; what it re-enters the VM to produce makes only the RESULT
/// unbounded, which is `Opaque`'s half of the declaration and seeds nothing
/// (docs/impl/region/effects.md § `Opaque`, the VM re-entry rule). The specifier
/// is read through a binding so the call keeps its opaque projection — a literal
/// spec is resolved and compiled at analysis time.
#[test]
fn import_does_not_seed_the_store_facet() {
    assert_binding_escape(
        "(let [s \"std/nonexistent\"] (length (import s)))",
        &[("s", false, false)],
    );
}

/// Container-read escape (the read-result → container-contents flow edge). A value
/// stored into a container and then read back OUT (`first`/`rest`/`get`/`pop`) and
/// ESCAPED must be marked escaping too: the ownership forest would otherwise adopt it
/// into the container's Owned subtree (a `%array-push` records `content ⊇ container`),
/// and the container's scope-exit subtree drop would then free a value that flows out
/// (`region_container_read_escape_uaf`, the UAF face; `store-wrapper`, the leak face).
///
/// The mark is PRECISE, not "every read escapes": the container's stored contents are
/// pulled into a facet ONLY when the read result itself reaches that facet, through the
/// ordinary fixpoint. A read whose result is consumed LOCALLY leaves the contents
/// interior — so a container that is merely read/indexed keeps its Owned reclamation.
#[test]
fn container_read_escape_spec() {
    // Pushed into a LOCAL container, read back out, and RETURNED: `v` escapes via the
    // read-through (the returned read result pulls the container's contents into the
    // return facet), so its region is a Shared seed and adoption is refused.
    assert_binding_escape(
        "(def f (fn (v) (let [a @[]] (%array-push a v) (first a)))) (f (list 1 2))",
        &[("v", true, true)],
    );
    // The SAME push, but the read result is consumed LOCALLY (`length`, non-escaping):
    // `v` stays interior — the precise gate, not "every element-read escapes".
    assert_binding_escape(
        "(def g (fn (v) (let [a @[]] (%array-push a v) (length (first a))))) (g (list 1 2))",
        &[("v", false, false)],
    );
    // `get` reads the same way: a value put into a local struct, read back by key and
    // returned, escapes; the container `m` (a store target) does not.
    assert_binding_escape(
        "(def h (fn (v) (let [m @{}] (%put m :k v) (get m :k)))) (h (list 1 2))",
        &[("v", true, true), ("m", false, false)],
    );
}
