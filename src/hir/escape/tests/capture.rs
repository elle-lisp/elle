use super::*;

/// The tightened capture facet (precision-point-3, applied transitively): a
/// closure captured by a capturer that is *called in place* does NOT escape, and
/// neither does the upvalue it holds. `inner` captures `up` and uses it
/// internally (`(length up)` — never returns it); `inner` is captured by `wrap`;
/// `wrap` is called in place. No closure crosses a frontier (return/store/fiber),
/// so the capture facet — pure transitive `lambda_captures` propagation from
/// genuine frontier seeds — marks nothing. The deleted `is_captured` producer
/// seed would have marked `inner` escaping merely because it is lexically
/// captured, dragging `up` with it; this pins that the proxy no longer seeds
/// escape.
#[test]
fn capture_by_called_in_place_closure_does_not_escape() {
    let src = "(def make (fn () (let [up \"x\"] \
                   (let [inner (fn () (length up))] \
                     (let [wrap (fn () (inner))] (wrap))))))";
    let (hir, arena, names, escape) = escape_of(src);
    // `up` is captured (by `inner`) but its capturer never escapes, so `up` must
    // not escape its activation.
    let up = bindings_named(&hir, &arena, &["up"], &names);
    assert!(!up.is_empty(), "missing `up` in `{src}`");
    assert!(
        up.iter().all(|b| !escape.binding_escapes_activation(*b)),
        "`up` captured by a called-in-place closure must NOT escape (the capture \
         facet is flow-true — no `is_captured` producer seed)"
    );
    // The lambda capturing `up` (`inner`) is itself called in place via `wrap`,
    // so it must not escape its definition.
    let names_up = |b: &Binding| names.get(&arena.get(*b).name.0).map(String::as_str) == Some("up");
    let mut found = false;
    for (lid, caps) in lambdas_with_captures(&hir) {
        if caps.iter().any(names_up) {
            found = true;
            assert!(
                !escape.lambda_escapes_definition(lid),
                "`inner` (captures `up`, called in place) must NOT escape its definition"
            );
        }
    }
    assert!(found, "no lambda capturing `up` found in `{src}`");
}

/// The transitive consumer's contract: every binding captured by an escaping
/// closure escapes too. Uses a shape where the captured value escapes ONLY via
/// capture — `c` is returned (escapes) and captures `p` but does not return or
/// store it (it uses it internally), so neither the return nor the store facet
/// reaches `p`.
#[test]
fn capture_consumer_escapes_upvalues_of_escaping_closures() {
    let src = "(def make (fn () (let [p \"x\"] (let [c (fn () (length p))] c))))";
    let (hir, arena, cc) = compile_with_cc(src);
    let escape = analyze_escape(&hir, &arena, &cc);

    let mut saw_escaping_capturer = false;
    for (lid, caps) in lambdas_with_captures(&hir) {
        if escape.lambda_escapes_definition(lid) {
            for b in caps {
                saw_escaping_capturer = true;
                assert!(
                    escape.binding_escapes_activation(b),
                    "binding {b:?} captured by escaping lambda #{} is not marked escaping",
                    lid.0
                );
            }
        }
    }
    assert!(
        saw_escaping_capturer,
        "test is vacuous: no escaping lambda with captures in `{src}`"
    );
}

/// Lexical capture is structural, not escape. `c` captures `p` but is called in
/// place (never escapes), and `p` is not returned or stored — so `p` does NOT
/// escape, even though the resolver marks it `is_captured`. This pins the
/// divergence: the analysis is finer than the `is_captured` proxy for captured
/// values (a regression that crudely equated capture with escape would mark `p`
/// and fail here).
#[test]
fn lexical_capture_is_not_escape() {
    let src = "(def f (fn () (let [p \"x\"] (let [c (fn () (length p))] (c)))))";
    let (hir, arena, cc) = compile_with_cc(src);
    let escape = analyze_escape(&hir, &arena, &cc);

    // Every binding in a lambda's capture set is, by construction, lexically
    // captured; we look for one whose value nonetheless does not escape — `p`,
    // captured by `c`, a closure called in place.
    let mut found = false;
    for (_lid, caps) in lambdas_with_captures(&hir) {
        for b in caps {
            if !escape.binding_escapes_activation(b) {
                found = true;
            }
        }
    }
    assert!(
        found,
        "expected a captured binding that does not escape (capture by a \
             non-escaping closure) in `{src}`"
    );
}

/// Fiber-boundary facet — a yielded heap binding crosses to the resumer, so it
/// escapes, even though it is neither returned (the body returns 0) nor stored
/// nor captured. The region solver has no compile-time fact for this
/// (`handle_emit` increfs at runtime), so it is verified structurally.
#[test]
fn fiber_emit_value_escapes() {
    let src = "(fiber/new (fn () (let [s \"x\"] (yield s) 0)) |:yield|)";
    let (hir, arena, cc) = compile_with_cc(src);
    let escape = analyze_escape(&hir, &arena, &cc);

    // The binding(s) yielded by an `Emit`-of-`Var` (through an optional
    // `DerefCell`).
    fn emitted_bindings(h: &Hir, out: &mut Vec<Binding>) {
        if let HirKind::Emit { value, .. } = &h.kind {
            let inner = match &value.kind {
                HirKind::DerefCell { cell } => &**cell,
                _ => &**value,
            };
            if let HirKind::Var(b) = &inner.kind {
                out.push(*b);
            }
        }
        h.for_each_child(|c| emitted_bindings(c, out));
    }
    let mut emitted = Vec::new();
    emitted_bindings(&hir, &mut emitted);
    assert!(
        !emitted.is_empty(),
        "no `Emit`-of-`Var` node found in `{src}`"
    );
    for b in emitted {
        assert!(
            escape.binding_escapes_activation(b),
            "yielded binding {b:?} crosses the fiber boundary but is not marked escaping"
        );
    }
}

/// Native-declared store escape, the precise *send* treatment. `chan/send`
/// declares `RegionEffect::Sends { args: [1] }` (a `Stores` that also crosses the
/// fiber frontier): its message (arg 1) is stored uncounted into the channel and
/// crosses the fiber boundary, so it escapes. The binding-level escape set treats
/// `Sends` exactly like `Stores`; the frontier distinction matters only for the
/// fiber facet (the Shared seed `regions::escape` projects).
/// The scrutiny the coarse presumption lacked is the *negative* case in the
/// same shape: the channel itself (arg 0) is the store target, not a source, so
/// it does NOT escape. The message is a **local** (`m`, a live region) — a
/// param message would be a phantom region the solver filters (the recorded
/// stored-borrowed-param divergence), which is a separate concern.
#[test]
fn native_store_arg_escapes_send_message_not_channel() {
    let src = "(def f (fn (c) (let [m \"hi\"] (chan/send c m))))";
    let mut symbols = crate::symbol::SymbolTable::new();
    let (hir, arena, names) = compile_fhir(src, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let escape = analyze_escape(&hir, &arena, &pc.call_classification);

    let m = bindings_named(&hir, &arena, &["m"], &names);
    let c = bindings_named(&hir, &arena, &["c"], &names);
    assert!(!m.is_empty() && !c.is_empty(), "missing m/c in `{src}`");
    // Positive: the message (the stored arg, a live local) escapes.
    assert!(
        m.iter().all(|b| escape.binding_escapes_activation(*b)),
        "chan/send message `m` is not marked escaping"
    );
    // Negative: the channel (the store target, not a source) does NOT escape.
    assert!(
        c.iter().all(|b| !escape.binding_escapes_activation(*b)),
        "chan/send channel `c` is wrongly marked escaping"
    );
}

/// Native-declared store/send escape under the real classification. `chan/send`
/// (`Sends`, a fiber-crossing store) escapes its message; `chan/recv` (`Fresh`)
/// and `fiber/new` (`Fresh`) escape nothing — the closure rides the fresh result.
#[test]
fn native_store_spec() {
    // chan/send stores+sends the message (arg 1): `m` escapes its activation; the
    // channel `c` (arg 0) is the target, not a source.
    assert_binding_escape(
        "(def f (fn (c) (let [m \"hi\"] (chan/send c m))))",
        &[("m", true, false), ("c", false, false)],
    );
    // chan/recv is Fresh — its argument is not stored.
    assert_binding_escape("(def f (fn (c) (chan/recv c)))", &[("c", false, false)]);
    // fiber/new is Fresh — `body` rides the fresh fiber result (no store edge), so
    // the `body` *binding* does not escape. But `body`'s own body IS `s`, so `s`
    // flows to that closure's tail and return-escapes (independent of fiber/new).
    assert_binding_escape(
        "(def f (fn () (let [s \"x\"] (let [body (fn () s)] (fiber/new body |:yield|)))))",
        &[("s", true, true), ("body", false, false)],
    );
}
