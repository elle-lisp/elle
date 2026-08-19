use super::*;

// ── native region effects (docs/impl/region/effects.md "Native region effects") ──
//
// A primitive's declared `RegionEffect` keys the opaque-call arg clique:
// Immediate/Fresh/PassThrough natives store no argument, so a call to one
// must record NO mutual may-store edges between its heap arguments; an
// undeclared (Mixed) native keeps the full clique (the conservative
// baseline — over-keep, never mis-free). A clique edge becomes a
// compile-time `IncrefRegion` balanced only by the target's free-time
// cascade IF the store actually happens — for a never-storing native the
// incref never balances (two leaked regions per call;
// tests/elle/region-native-effect-clique-leak.lisp).

#[test]
fn effect_immediate_call_emits_no_arg_clique() {
    // `identical?` declares Immediate: returns a bool, stores nothing.
    // The call must record NO may-store edges between its two heap
    // (string-literal) arguments.
    let (hir, arena, symbols, info) = analyze_with_class("(identical? \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "identical?", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (identical? ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "an Immediate native call must not record arg-clique edges; got {:?}",
        edges
    );
}

#[test]
fn effect_mixed_call_keeps_arg_clique() {
    // `git` is declared `Mixed`, and genuinely so: it hands its closure argument to
    // the GPU compile path and caches the compiled SPIR-V on that closure's
    // template — a retention no compile-time seam records — so the conservative full
    // mutual may-store clique between its two heap args is the right answer
    // (over-keep, never mis-free). Tests the REAL classification (a primitive
    // *declared* Mixed → clique), the boundary against over-deletion — not a forced
    // effect, which would merely re-exercise the solver's Mixed arm already covered
    // by `effect_unknown_call_keeps_arg_clique`.
    let (hir, arena, symbols, info) = analyze_with_class("(git \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "git", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (git ...) call");
    let edges = edges_at_site(&info, calls[0]);
    let mutual = edges
        .iter()
        .any(|&(src, dst)| edges.contains(&(dst, src)) && src != dst);
    assert!(
        mutual,
        "a Mixed native call must keep the mutual arg clique; got {:?}",
        edges
    );
}

/// The clique is over pairs of ARGUMENTS: a `Mixed` native reached with ONE heap
/// argument records no edge, however many source regions that argument's value
/// carries. `k` here has one per arm of its `if`, and the two are alternatives
/// for the single value `git` receives — never two values one could store into
/// the other — so an edge between them would be an `IncrefRegion` no free cascade
/// balances (docs/impl/region/effects.md § "What the solver derives"; the rate is
/// pinned by tests/elle/region-native-effect-clique-leak.lisp).
///
/// The declarant must be one that is genuinely `Mixed`, or the test would assert
/// nothing about the clique loop: `git` caches compiled SPIR-V on its argument's
/// template, a retention no compile-time seam records.
#[test]
fn effect_mixed_call_pairs_arguments_not_one_arguments_regions() {
    let (hir, arena, symbols, info) =
        analyze_with_class("(let [k (if (%lt 1 0) (fn () 1) (fn () 2))] (git k))");
    let calls = find_calls_to_primitive(&hir, "git", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (git ...) call");
    let reader = find_binding_by_name(&hir, "k", &arena, &symbols).expect("the reader binding k");
    assert!(
        info.binding_source_regions
            .get(&reader)
            .is_some_and(|rs| rs.len() >= 2),
        "precondition: the branch gives k a region per arm (got {:?})",
        info.binding_source_regions.get(&reader),
    );
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "a single-argument Mixed call must record no clique edge; got {edges:?}",
    );
    assert!(
        info.hard_edge_sites.contains(&calls[0]),
        "the declarant must still be Mixed, or this shape asserts nothing about \
         the clique loop"
    );
}

#[test]
fn effect_delivers_call_emits_no_arg_clique() {
    // `fiber/resume` is declared `Delivers { args: [1] }`: it installs the resume
    // value into the target fiber's signal slot, a seam that counts its own
    // reference — the park-retain and its recorded `fiber → signal` edge for an
    // install that outlives the call, a transient handover the resume consumes
    // otherwise. So the call records NO may-store edge, exactly as `Funnel` does for
    // the mutable-store funnel; a compile-time incref would never balance
    // (tests/elle/region-fiber-install-clique-leak.lisp). Uses the REAL
    // classification, so a regression that re-declares an installer `Mixed` fails
    // here as well as on the rate.
    let (hir, arena, symbols, info) = analyze_with_class("(fiber/resume \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "fiber/resume", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (fiber/resume ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "a Delivers native call must not record arg-clique edges; got {:?}",
        edges
    );
}

#[test]
fn effect_unknown_call_keeps_arg_clique() {
    // Unknown ("nobody has looked" — the default for unexamined
    // primitives, plugin definitions, and user-supplied functions) is
    // operationally identical to Mixed: the full mutual clique.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, symbols, info) =
        analyze_with_effect("(string \"a\" \"b\")", "string", RegionEffect::Unknown);
    let calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (string ...) call");
    let edges = edges_at_site(&info, calls[0]);
    let mutual = edges
        .iter()
        .any(|&(src, dst)| edges.contains(&(dst, src)) && src != dst);
    assert!(
        mutual,
        "an Unknown native call must keep the mutual arg clique; got {:?}",
        edges
    );
}
#[test]
fn effect_fresh_call_emits_no_arg_clique() {
    // Fresh: the result is freshly allocated, no argument is stored —
    // no may-store edges between heap args.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, symbols, info) =
        analyze_with_effect("(string \"a\" \"b\")", "string", RegionEffect::Fresh);
    let calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(calls.len(), 1);
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "a Fresh native call must not record arg-clique edges; got {:?}",
        edges
    );
}

#[test]
fn effect_passthrough_call_emits_no_arg_clique() {
    // PassThrough: the result lives in an argument's region, no argument
    // is stored — no may-store edges; the dispatch pass-through retain
    // carries the result's lifetime at runtime.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, symbols, info) =
        analyze_with_effect("(string \"a\" \"b\")", "string", RegionEffect::PassThrough);
    let calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(calls.len(), 1);
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "a PassThrough native call must not record arg-clique edges; got {:?}",
        edges
    );
}

#[test]
fn effect_stores_call_emits_directed_edges_only() {
    // Stores{args: [0]}: a directed may-store edge from the stored
    // argument's region to each OTHER heap argument's regions — and
    // nothing else. No reverse edges (unlike the Mixed/Unknown mutual
    // clique), no edges among the non-stored arguments.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, symbols, info) = analyze_with_effect(
        "(string \"a\" \"b\" \"c\")",
        "string",
        RegionEffect::Stores { args: &[0] },
    );
    let calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(calls.len(), 1);
    let r_a = string_literal_region(&hir, &info, "a");
    let r_b = string_literal_region(&hir, &info, "b");
    let r_c = string_literal_region(&hir, &info, "c");
    let mut edges = edges_at_site(&info, calls[0]);
    edges.sort_by_key(|&(s, d)| (s.0, d.0));
    let mut expected = vec![(r_a, r_b), (r_a, r_c)];
    expected.sort_by_key(|&(s, d)| (s.0, d.0));
    assert_eq!(
        edges, expected,
        "Stores{{args: [0]}} must record exactly the directed edges from \
         the stored arg's region r{} to the other heap args' regions \
         (r{}, r{})",
        r_a.0, r_b.0, r_c.0
    );
}
#[test]
fn effect_sends_call_emits_no_arg_clique() {
    // `Sends{args}` is seam-counted, exactly like `Delivers`: the send body
    // retains the message's region at runtime after a successful enqueue
    // (`EscapeSite::ChanSend` in `prim_chan_send`), and the receive lowers it
    // (`release_received_message`). So the call records NO may-store edge. A
    // compile-time edge is doubly wrong here: it double-counts against the
    // receive's single release where its region pair is nameable, and it silently
    // fails to fire where the channel is an upvalue or module-level binding (no
    // pair to key the incref on) — the owned-parameter message UAF
    // (tests/elle/region-chan-send-owned-param-uaf.lisp). The fiber-frontier
    // *escape* of a `Sends` message is escape's judgment, not a solver-recorded
    // seed — pinned in the escape tests (`native_store_spec`, the real
    // `chan/send`). Same shape and harness as
    // `effect_stores_call_emits_directed_edges_only`, with `string` declared
    // `Sends`.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, symbols, info) = analyze_with_effect(
        "(string \"a\" \"b\" \"c\")",
        "string",
        RegionEffect::Sends { args: &[0] },
    );
    let calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(calls.len(), 1);
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "a Sends native call must not record arg-clique edges (the send seam \
         counts its own reference); got {:?}",
        edges
    );
    assert!(
        !info.hard_edge_sites.contains(&calls[0]),
        "a Sends call site records no edges, so it must not be marked hard"
    );
}

#[test]
fn hard_edge_sites_marks_native_uncounted_store_sites() {
    // `git` is declared `Mixed` (it caches compiled SPIR-V on its closure argument's
    // template, a retention no compile-time seam records), so its clique edges are
    // HARD — the lowerer emits the incref value-based for a call-result source
    // (docs/impl/region/effects.md "Hard edges: how a may-store edge is emitted"). Pins
    // the inclusion side of the hard/soft split, the Mixed companion of
    // `hard_edge_sites_marks_declared_stores_sites`, through the REAL classification.
    let (hir, arena, symbols, info) = analyze_with_class("(git \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "git", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (git ...) call");
    assert!(
        info.hard_edge_sites.contains(&calls[0]),
        "a Mixed native call site must be a hard-edge site"
    );
}

#[test]
fn hard_edge_sites_marks_declared_stores_sites() {
    // A declared Stores site is hard for the same reason Mixed is: the
    // store is real and uncounted at compile time.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, symbols, info) = analyze_with_effect(
        "(string \"a\" \"b\")",
        "string",
        RegionEffect::Stores { args: &[0] },
    );
    let calls = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(calls.len(), 1);
    assert!(
        info.hard_edge_sites.contains(&calls[0]),
        "a declared Stores native call site must be a hard-edge site"
    );
}

#[test]
fn port_write_declares_immediate_no_arg_clique() {
    // Real-primitive companion to `effect_immediate_call_emits_no_arg_clique`
    // (which forces the effect on `string`): this pins `port/write`'s ACTUAL
    // declaration. `port/write` takes two heap args (port + data) but stores
    // neither — it writes the bytes to a file descriptor and yields an integer
    // byte count — so it is `Immediate` and its call records NO arg-clique
    // edges. Under the prior `Mixed` declaration the mutual clique increfed
    // both heap args' regions, and since nothing is stored the increfs never
    // balanced: one leaked region per call for a freshly-materialized data arg
    // (region-port-write-effect.lisp documents the runtime side). The yielding
    // result side is oracle-exempt (a SIG_YIELD return is not a normal
    // completion), so the declaration's clique effect is what guards the leak —
    // a regression back to Mixed reintroduces it and goes RED here.
    let (hir, arena, symbols, info) = analyze_with_class("(port/write \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "port/write", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (port/write ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "port/write declares Immediate, so its call must record no arg-clique \
         edges; got {:?} (a regression to Mixed — the data-region leak)",
        edges
    );
}

#[test]
fn udp_send_to_declares_immediate_no_arg_clique() {
    // `udp/send-to(socket data addr port)` takes THREE heap args (socket +
    // data + addr string) but stores none into another — it ships `data` into
    // the kernel and copies `addr` out to a Rust String — and the io completion
    // returns `Value::int(result_code)`. So it is `Immediate`: its call records
    // NO arg-clique edges. Under `Mixed` the full mutual clique (three pairs)
    // increfed all three regions, never balanced (nothing stored) — a per-call
    // leak. Yielding, so oracle-exempt; the declaration's clique effect is the
    // guard. Sibling of `port_write_declares_immediate_no_arg_clique`, with a
    // wider clique (the >2-heap-arg case). RED under a regression to Mixed.
    let (hir, arena, symbols, info) = analyze_with_class("(udp/send-to \"s\" \"d\" \"a\" 9000)");
    let calls = find_calls_to_primitive(&hir, "udp/send-to", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (udp/send-to ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "udp/send-to declares Immediate, so its call must record no arg-clique \
         edges; got {:?} (a regression to Mixed — the three-arg-clique leak)",
        edges
    );
}

#[test]
fn subprocess_exec_declares_opaque_no_arg_clique() {
    // `subprocess/exec(program args [opts])` takes multiple heap args (program
    // string + args list + opts struct) but copies every one out — program/args/
    // env into a Rust `SpawnRequest` (`String`/`Vec`) — and stores none into
    // another, while returning an OPAQUE result minted on the scheduler heap (the
    // `{:pid … :process}` struct, neither the call's own region nor an arg's). So
    // it is `Opaque`, not `Mixed`: it records NO arg-clique edges. Under `Mixed`
    // the full mutual clique increfed every heap arg's region and never balanced
    // (nothing is stored) — a per-call leak on a no-store primitive, exactly the
    // gap `Opaque` closes (docs/impl/region/effects.md § Opaque: the clique is
    // keyed on the store, not the result shape). RED under a regression to Mixed.
    let (hir, arena, symbols, info) =
        analyze_with_class("(subprocess/exec \"echo\" (list \"hi\"))");
    let calls = find_calls_to_primitive(&hir, "subprocess/exec", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (subprocess/exec ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "subprocess/exec declares Opaque, so its call must record no arg-clique \
         edges; got {:?} (a regression to Mixed — the no-store clique leak)",
        edges
    );
}

#[test]
fn has_declares_opaque_no_arg_clique() {
    // `has?` resolves its work through the value's trait table, so its RESULT is
    // unbounded: `with-traits` may replace `:Collection` with a user closure returning
    // anything, and neither `Immediate` nor `Fresh` holds on every path. Its STORE side
    // is bounded regardless — the built-in `Collection:has?` reads and returns a bool,
    // and a user closure is ordinary Elle code, which stores only through the
    // runtime-counted mutable-store funnel. Unbounded result + no store is `Opaque`, so
    // the call records NO arg-clique edges. Under `Mixed` the mutual clique increfed
    // both heap args' regions and never balanced (nothing is stored) — two leaked
    // regions per call (tests/elle/region-has-clique-leak.lisp). The sibling of
    // `subprocess_exec_declares_opaque_no_arg_clique` on the trait-dispatch face; RED
    // under a regression to Mixed.
    let (hir, arena, symbols, info) = analyze_with_class("(has? \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "has?", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (has? ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "has? declares Opaque, so its call must record no arg-clique edges; \
         got {:?} (a regression to Mixed — the trait-dispatch clique leak)",
        edges
    );
    assert!(
        !info.hard_edge_sites.contains(&calls[0]),
        "an Opaque call site must NOT be a hard-edge site"
    );
}

#[test]
fn fiber_child_declares_opaque_and_propagate_keeps_the_hard_edge() {
    // The two fiber-graph natives split on the STORE side, and the split is what
    // their declarations record. `fiber/child` reads the cached child-fiber `Value`
    // out of its argument and returns it — no store, and a result living in whatever
    // region the resume minted the cache in, so `Opaque`: no hard-edge site, and no
    // store-facet escape seed on the argument (the escape half is
    // `a_fiber_graph_read_does_not_seed_the_store_facet`). `fiber/propagate` returns
    // SIG_PROPAGATE, which drives the VM to write its argument into the propagating
    // fiber's own `child`/`child_value` fields with no counting seam — the uncounted
    // store the clique covers — so it stays `Mixed` and stays a hard-edge site.
    // Both are single-heap-arg, so the clique edge set is empty either way and
    // `hard_edge_sites` is what distinguishes them (docs/impl/region/effects.md
    // § `Opaque`, § `Mixed`).
    let (hir, arena, symbols, info) = analyze_with_class("(fiber/child \"f\")");
    let calls = find_calls_to_primitive(&hir, "fiber/child", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (fiber/child ...) call");
    assert!(
        !info.hard_edge_sites.contains(&calls[0]),
        "fiber/child declares Opaque — it stores nothing — so its call site must \
         NOT be a hard-edge site (a regression to Mixed re-seeds the store facet \
         on every fiber it reads)"
    );

    let (hir, arena, symbols, info) = analyze_with_class("(fiber/propagate \"f\")");
    let calls = find_calls_to_primitive(&hir, "fiber/propagate", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (fiber/propagate ...) call");
    assert!(
        info.hard_edge_sites.contains(&calls[0]),
        "fiber/propagate's argument is stored uncounted into the propagating \
         fiber's child field, so it must stay a Mixed hard-edge site"
    );
}

#[test]
fn import_declares_opaque_no_hard_edge() {
    // `import` copies its specifier out to a Rust `String` to resolve it and stores
    // no argument; the module value it hands back is produced by compiled code run
    // on the driving VM, so the RESULT is unbounded and nothing else is — the VM
    // re-entry rule's answer, `Opaque` (docs/impl/region/effects.md § `Opaque`).
    // Single-heap-arg, so the clique is empty either way: `hard_edge_sites` and the
    // store-facet seed (`import_does_not_seed_the_store_facet`) are what a
    // regression to Mixed brings back. The result stays non-fresh — it lives in
    // neither the call's own region nor the specifier's.
    let (hir, arena, symbols, info) = analyze_with_class("(import \"std/nonexistent\")");
    let calls = find_calls_to_primitive(&hir, "import", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (import ...) call");
    assert!(
        !info.hard_edge_sites.contains(&calls[0]),
        "import declares Opaque, so its call site must NOT be a hard-edge site"
    );
    let call_r = *info
        .alloc_region
        .get(&calls[0])
        .expect("import call must have a call-result region");
    assert!(
        !info.fresh_result_regions.contains(&call_r),
        "import's result is minted by the module's own compiled top level, not in \
         the call's region, so r{} must not be a fresh result",
        call_r.0
    );
}

#[test]
fn io_yield_pass_tightenings_drop_the_mixed_hard_edge() {
    // The io / fiber pass (docs/impl/region/effects.md "Native region effects").
    // Every primitive
    // here yields (`SIG_YIELD|SIG_IO`) or returns a signal, so the result-side
    // declaration ORACLE is exempt — it never panics on an over-claim. This
    // solver counterfactual is the guard instead:
    //
    //   * a tightened native must NOT be a `hard_edge_sites` member — only
    //     Mixed/Unknown insert one (walkrest.rs's Mixed arm; region/effects.md
    //     "What the solver derives"). Every call below is single-heap-arg, so the
    //     clique edge set is already empty under both Mixed and the tightened
    //     effect — `hard_edge_sites`, NOT `edges_at_site`, is what distinguishes
    //     them.
    //   * its call-result region appears in `fresh_result_regions` IFF declared
    //     `Fresh` (walkrest.rs's Fresh arm seeds the Stage-6 Owned candidate).
    //
    // Both flip RED under a regression to `Mixed`: Mixed re-adds the hard edge
    // and drops the fresh marking. (The ≥2-heap-arg leak declarants port/write and
    // udp/send-to have their own edge-shape tests above.)
    use crate::primitives::def::RegionEffect;
    let cases: &[(&str, &str, RegionEffect)] = &[
        // → Fresh: a buffer/port/struct pre-minted in the call's own region and
        //   filled in place, or a freshly-built array/value in that region.
        ("(port/read \"p\" 1)", "port/read", RegionEffect::Fresh),
        (
            "(port/read-line \"p\")",
            "port/read-line",
            RegionEffect::Fresh,
        ),
        (
            "(port/read-exact \"p\" 1)",
            "port/read-exact",
            RegionEffect::Fresh,
        ),
        ("(port/open \"f\" :read)", "port/open", RegionEffect::Fresh),
        (
            "(port/open-bytes \"f\" :read)",
            "port/open-bytes",
            RegionEffect::Fresh,
        ),
        ("(tcp/accept \"l\")", "tcp/accept", RegionEffect::Fresh),
        (
            "(tcp/connect-ip \"127.0.0.1\" 80)",
            "tcp/connect-ip",
            RegionEffect::Fresh,
        ),
        ("(unix/accept \"l\")", "unix/accept", RegionEffect::Fresh),
        ("(unix/connect \"p\")", "unix/connect", RegionEffect::Fresh),
        (
            "(udp/recv-from \"s\" 64)",
            "udp/recv-from",
            RegionEffect::Fresh,
        ),
        (
            "(chan/wait-ready \"c\")",
            "chan/wait-ready",
            RegionEffect::Fresh,
        ),
        ("(fiber/parent \"x\")", "fiber/parent", RegionEffect::Fresh),
        ("(io/wait \"b\" 10)", "io/wait", RegionEffect::Fresh),
        // → Immediate: nil or int result.
        ("(port/flush \"p\")", "port/flush", RegionEffect::Immediate),
        ("(port/close \"p\")", "port/close", RegionEffect::Immediate),
        ("(port/seek \"p\" 0)", "port/seek", RegionEffect::Immediate),
        ("(port/tell \"p\")", "port/tell", RegionEffect::Immediate),
        (
            "(tcp/shutdown \"p\" :write)",
            "tcp/shutdown",
            RegionEffect::Immediate,
        ),
        (
            "(unix/shutdown \"p\" :write)",
            "unix/shutdown",
            RegionEffect::Immediate,
        ),
        ("(ev/sleep 1)", "ev/sleep", RegionEffect::Immediate),
        (
            "(ev/poll-fd 1 :read)",
            "ev/poll-fd",
            RegionEffect::Immediate,
        ),
        (
            "(subprocess/wait \"h\")",
            "subprocess/wait",
            RegionEffect::Immediate,
        ),
        // → Opaque: stores nothing, result minted at completion on the origin heap.
        (
            "(port/read-all \"p\")",
            "port/read-all",
            RegionEffect::Opaque,
        ),
        ("(sys/resolve \"h\")", "sys/resolve", RegionEffect::Opaque),
        ("(os/sig-next \"r\")", "os/sig-next", RegionEffect::Opaque),
        ("(watch-next \"w\")", "watch-next", RegionEffect::Opaque),
    ];
    for (src, prim, effect) in cases {
        let (hir, arena, symbols, info) = analyze_with_class(src);
        let calls = find_calls_to_primitive(&hir, prim, &arena, &symbols);
        assert_eq!(
            calls.len(),
            1,
            "expected exactly one ({} ...) call in {:?}",
            prim,
            src
        );
        let site = calls[0];
        assert!(
            !info.hard_edge_sites.contains(&site),
            "{} is declared {:?}, so its call site must NOT be a hard-edge site — \
             a regression to Mixed re-adds it (the spurious uncounted-store clique)",
            prim,
            effect,
        );
        let call_r = *info
            .alloc_region
            .get(&site)
            .unwrap_or_else(|| panic!("{} call must have a call-result region", prim));
        let is_fresh = info.fresh_result_regions.contains(&call_r);
        if matches!(effect, RegionEffect::Fresh) {
            assert!(
                is_fresh,
                "{} is declared Fresh, so its call-result region r{} must be in \
                 fresh_result_regions (the Stage-6 Owned candidate); a regression \
                 to Mixed drops it",
                prim, call_r.0,
            );
        } else {
            assert!(
                !is_fresh,
                "{} is declared {:?} (non-Fresh), so its call-result region r{} must \
                 NOT be in fresh_result_regions",
                prim, effect, call_r.0,
            );
        }
    }
}

#[test]
fn userfn_call_site_records_no_arg_clique() {
    // `h` is a function-valued parameter — a genuinely opaque user fn
    // (no `binding_lambda` entry, so `try_inline_call` bails; not a
    // primitive name, so `call_effect` is None). A user fn is ordinary
    // Elle code: it can store an argument into a mutable container ONLY
    // through the runtime-counted mutable-store funnel (Rule 5,
    // statically complete) or via a counted edge in its OWN compilation,
    // so a caller-side clique incref is pure redundancy that leaks one
    // region per alloc-region heap argument per call (pinned by
    // region-userfn-clique-noleak.lisp). So a `None`-effect call records
    // NO arg-clique edges at all — distinct from a Mixed/Unknown NATIVE,
    // which can store uncounted and keeps the full clique. The site is of
    // course also NOT a hard-edge site (only declared natives are).
    // (docs/impl/region/effects.md "What the solver derives", the
    // user-functions case.)
    let (hir, arena, symbols, info) = analyze_with_class("((fn (h) (h \"a\" \"b\")) f)");
    let calls = find_calls_to_primitive(&hir, "h", &arena, &symbols);
    assert_eq!(calls.len(), 1, "expected one (h ...) call");
    let edges = edges_at_site(&info, calls[0]);
    assert!(
        edges.is_empty(),
        "an opaque user-fn call must record NO arg-clique edges; got {:?}",
        edges
    );
    assert!(
        !info.hard_edge_sites.contains(&calls[0]),
        "a user-fn call site must NOT be a hard-edge site"
    );
}
