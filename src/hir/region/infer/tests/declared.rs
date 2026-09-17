//! audited: 2026-09-17
//! What each shipped primitive declares, held to what the solver then does —
//! the real-primitive companions to the variant tests in effects.rs.
//!
//! docs/impl/region/clique.md

use super::*;

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
    let (hir, arena, _symbols, info) = analyze_with_class("(port/write \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "port/write", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_class("(udp/send-to \"s\" \"d\" \"a\" 9000)");
    let calls = find_calls_to_primitive(&hir, "udp/send-to", &arena);
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
    // `subprocess`, in neither the call's own region nor an arg's). So
    // it is `Opaque`, not `Mixed`: it records NO arg-clique edges. Under `Mixed`
    // the full mutual clique increfed every heap arg's region and never balanced
    // (nothing is stored) — a per-call leak on a no-store primitive, exactly the
    // gap `Opaque` closes (docs/impl/region/effects.md § Opaque: the clique is
    // keyed on the store, not the result shape). RED under a regression to Mixed.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(subprocess/exec \"echo\" (list \"hi\"))");
    let calls = find_calls_to_primitive(&hir, "subprocess/exec", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_class("(has? \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "has?", &arena);
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
fn fiber_graph_natives_declare_opaque_and_git_keeps_the_hard_edge() {
    // Both fiber-graph natives touch the same `child`/`child_value` pair and both
    // declare `Opaque`. `fiber/child` READS the cached child-fiber `Value` out of
    // its argument; `fiber/propagate` returns SIG_PROPAGATE, which drives the VM to
    // WRITE its argument into that pair. Neither is a store: the free-time walk's
    // Fiber arm does not enumerate the child chain, so the field creates no holder
    // of the region and can under-count nothing (docs/impl/region/effects.md
    // § `Opaque`, "The child-chain WIRING is `Opaque` too"). The escape half is
    // `a_fiber_graph_write_does_not_seed_the_store_facet`.
    for name in ["fiber/child", "fiber/propagate"] {
        let (hir, arena, _symbols, info) = analyze_with_class(&format!("({name} \"f\")"));
        let calls = find_calls_to_primitive(&hir, name, &arena);
        assert_eq!(calls.len(), 1, "expected one ({name} ...) call");
        assert!(
            !info.hard_edge_sites.contains(&calls[0]),
            "{name} declares Opaque — it stores nothing — so its call site must NOT \
             be a hard-edge site (a regression to Mixed re-seeds the store facet on \
             every fiber it names)"
        );
    }

    // The contrast that keeps the handler-store rule honest. `git` also does its
    // work through a signal handler, and that handler caches the compiled SPIR-V on
    // its closure argument's template — a retention outliving the call that no seam
    // records. Real, uncounted store: `Mixed`, and a hard-edge site. All three are
    // single-heap-arg, so the clique edge set is empty for each and `hard_edge_sites`
    // is the only thing that separates them.
    let (hir, arena, _symbols, info) = analyze_with_class("(git \"f\")");
    let calls = find_calls_to_primitive(&hir, "git", &arena);
    assert_eq!(calls.len(), 1, "expected one (git ...) call");
    assert!(
        info.hard_edge_sites.contains(&calls[0]),
        "git's argument keeps compiled SPIR-V cached on its template past the call, \
         so it must stay a Mixed hard-edge site"
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
    let (hir, arena, _symbols, info) = analyze_with_class("(import \"std/nonexistent\")");
    let calls = find_calls_to_primitive(&hir, "import", &arena);
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
        let (hir, arena, _symbols, info) = analyze_with_class(src);
        let calls = find_calls_to_primitive(&hir, prim, &arena);
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
