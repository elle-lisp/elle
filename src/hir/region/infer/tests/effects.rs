//! audited: 2026-09-17
//! Each RegionEffect variant, held to the arg-clique edges it should and
//! should not produce.
//!
//! docs/impl/region/clique.md

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
    let (hir, arena, _symbols, info) = analyze_with_class("(identical? \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "identical?", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_class("(git \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "git", &arena);
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
    let (hir, arena, _symbols, info) =
        analyze_with_class("(let [k (if (%lt 1 0) (fn () 1) (fn () 2))] (git k))");
    let calls = find_calls_to_primitive(&hir, "git", &arena);
    assert_eq!(calls.len(), 1, "expected one (git ...) call");
    let reader = find_binding_by_name(&hir, "k", &arena).expect("the reader binding k");
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
    let (hir, arena, _symbols, info) = analyze_with_class("(fiber/resume \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "fiber/resume", &arena);
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
    // Unknown ("nobody has looked" — the default for unexamined primitives and
    // plugin definitions) is operationally identical to Mixed: the full mutual
    // clique. A user function is NOT Unknown; it records no clique at all, as
    // `userfn_call_site_records_no_arg_clique` below asserts.
    use crate::primitives::def::RegionEffect;
    let (hir, arena, _symbols, info) =
        analyze_with_effect("(string \"a\" \"b\")", "string", RegionEffect::Unknown);
    let calls = find_calls_to_primitive(&hir, "string", &arena);
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
    let (hir, arena, _symbols, info) =
        analyze_with_effect("(string \"a\" \"b\")", "string", RegionEffect::Fresh);
    let calls = find_calls_to_primitive(&hir, "string", &arena);
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
    let (hir, arena, _symbols, info) =
        analyze_with_effect("(string \"a\" \"b\")", "string", RegionEffect::PassThrough);
    let calls = find_calls_to_primitive(&hir, "string", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_effect(
        "(string \"a\" \"b\" \"c\")",
        "string",
        RegionEffect::Stores { args: &[0] },
    );
    let calls = find_calls_to_primitive(&hir, "string", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_effect(
        "(string \"a\" \"b\" \"c\")",
        "string",
        RegionEffect::Sends { args: &[0] },
    );
    let calls = find_calls_to_primitive(&hir, "string", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_class("(git \"a\" \"b\")");
    let calls = find_calls_to_primitive(&hir, "git", &arena);
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
    let (hir, arena, _symbols, info) = analyze_with_effect(
        "(string \"a\" \"b\")",
        "string",
        RegionEffect::Stores { args: &[0] },
    );
    let calls = find_calls_to_primitive(&hir, "string", &arena);
    assert_eq!(calls.len(), 1);
    assert!(
        info.hard_edge_sites.contains(&calls[0]),
        "a declared Stores native call site must be a hard-edge site"
    );
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
    let (hir, arena, _symbols, info) = analyze_with_class("((fn (h) (h \"a\" \"b\")) f)");
    let calls = find_calls_to_primitive(&hir, "h", &arena);
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
