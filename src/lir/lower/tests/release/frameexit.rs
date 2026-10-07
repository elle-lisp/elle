// audited: 2026-10-06
//! A release past a frame-replacing tail call is carried ahead of it, unless it is the call's ownership move.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/relocate.md

use super::*;

// ── The frame-exit release ───────────────────────────────────────
// Everything the lowerer emits after a `TailCall` runs only on the NATIVE
// fall-through — a native pushes no bytecode frame and the dispatch loop
// continues into that block, while a closure callee replaces the frame and never
// arrives. For the call's own arguments that is the ownership move; for anything
// else it strands the frame's reference, so the release is carried back ahead of
// the `TailCall` (docs/impl/region/mechanism.md § "A release past a
// frame-replacing tail call is not a release"). These pin the PLACEMENT: the
// counts are unchanged either way, so only position can tell the two apart.

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefValueRegion`s. `None` if no block has a
/// `TailCall`.
fn tail_call_release_layout(module: &FrozenModule) -> Option<(usize, Vec<usize>)> {
    let (b, at) = first_tail_call_block(module)?;
    Some((
        at,
        positions(&b, |i| matches!(i, InstrRef::DecrefValueRegion { .. })),
    ))
}

#[test]
fn stranded_param_release_precedes_the_frame_replacing_tail_call() {
    // `x` is used nowhere, so its release is the unused-parameter fallback the
    // lowerer emits at the end of the body — the dead block. It must be carried
    // back ahead of the `TailCall`, or the moved-in argument is stranded once per
    // call (the `tail-frame-exit-unused` probe).
    let module = compile_to_lir("(begin (def s (fn () 0)) (def f (fn (x) (s))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the unused parameter's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn moved_argument_release_stays_after_the_tail_call() {
    // The exemption, and the over-free face of the same placement: `x` IS the
    // tail call's argument, so its never-executed release is the transfer the
    // callee's owned-param release consumes. Hoisting it would drop the
    // reference the callee now owns.
    let module = compile_to_lir("(begin (def s (fn (a) a)) (def f (fn (x) (s x))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        !releases.is_empty() && releases.iter().all(|&r| r > at),
        "a moved argument's release was hoisted ahead of the TailCall \
         (at={at}, releases={releases:?}) — that release IS the ownership move",
    );
}

#[test]
fn captured_param_release_precedes_the_frame_replacing_tail_call() {
    // The tail callee reaches `x` through its CAPTURED environment, which no
    // argument names — and the release is hoisted anyway, because building the
    // env took a counted reference through the allocation funnel, so the frame's
    // own is still the only one this drops (docs/impl/region/mechanism.md §
    // "Lexical capture is not a second holder to fear"; the
    // `tail-frame-exit-captured` probe).
    let module =
        compile_to_lir("(begin (def f (fn (x) (let [g (fn () (%int? x))] (g)))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the captured parameter's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn capture_handed_back_by_the_callee_precedes_the_tail_call() {
    // The tail callee hands `x` BACK, so the caller's owning reference is minted
    // by the CALLEE's `Return`, after this release runs. The release is hoisted
    // anyway, because the same capture that lets `g` read `x` is a counted edge
    // that outlives the mint — it falls away only with `g`'s region, at the
    // callee's completion (docs/impl/region/mechanism.md § "The callee's return
    // mint, and why the point owes it nothing"; the `tail-frame-exit-handback`
    // probe). This is the stdlib walker's accumulator.
    let module = compile_to_lir("(begin (def f (fn (x) (let [g (fn () x)] (g)))) (f (list 1 2)))");
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the handed-back capture's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

#[test]
fn handback_the_callee_cannot_reach_precedes_the_tail_call() {
    // The other end of the same enumeration. `x` reaches a return through the OTHER
    // arm, so it is on the return frontier — and the arm that leaves through a
    // frame-replacing callee calls one that neither names nor captures it. A callee
    // reaches a value this frame owns by those two routes and no other, so this one
    // cannot mint against `x`'s region at all and the hoisted release is the last
    // (docs/impl/region/mechanism.md § "The callee's return mint, and why the point
    // owes it nothing").
    // `s` is int-valued so that the first `TailCall`-bearing function is `f` itself
    // — a callee whose own body tail-calls a native would be read instead, and its
    // layout says nothing about this placement.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) (def f (fn (x c) (if c x (s)))) (f (list 1 2) false))",
    );
    let (at, releases) = tail_call_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the hand-back's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefRegion`s — the slot-resolved twin of
/// [`tail_call_release_layout`], which reads the value route. A self-recursive
/// closure's region is released by id, so only this reading sees it.
fn tail_call_region_release_layout(module: &FrozenModule) -> Option<(usize, Vec<usize>)> {
    let (b, at) = first_tail_call_block(module)?;
    Some((
        at,
        positions(&b, |i| matches!(i, InstrRef::DecrefRegion { .. })),
    ))
}

#[test]
fn region_an_argument_only_called_is_released_before_the_tail_call() {
    // The exemption reads an operand's VALUE, not its syntax
    // (docs/impl/region/mechanism.md § "What an operand names is its VALUE, not its
    // syntax"). `go` is named nowhere in the tail call — its ARGUMENT calls `go`,
    // so what `helper` is handed is that call's RESULT, and `go`'s own closure
    // region was read and finished with beforehand. Its release sits at the
    // letrec's scope end, past the `TailCall`, and must be carried back.
    let module = compile_to_lir(
        "(begin (def f (fn (n) \
         (letrec [helper (fn (x) (%sub x 1)) \
                   go (fn (m) (if (%lt m 1) 0 (go (%sub m 1))))] \
           (helper (go n))))) (f 3))",
    );
    let (at, releases) =
        tail_call_region_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the region an argument's own call named is still released after the \
         TailCall (at={at}, releases={releases:?}) — dead on the closure path",
    );
}

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefRegion`s naming the region `of` picks out of the
/// same block's allocating instructions.
///
/// Reading by REGION rather than by instruction count is what makes a decline pin
/// specific: a block releases several regions around its tail call, so "some
/// release precedes it" says nothing about which one did.
fn tail_call_slot_release_layout(
    module: &FrozenModule,
    of: impl Fn(&InstrRef<'_>) -> Option<StaticRegion>,
) -> Option<(usize, Vec<usize>)> {
    tail_call_blocks(module).find_map(|(b, at)| {
        let want = b.instrs().find_map(|i| of(&i))?;
        Some((
            at,
            positions(
                &b,
                |i| matches!(i, InstrRef::DecrefRegion { region_id } if *region_id == want),
            ),
        ))
    })
}

#[test]
fn container_of_an_opcode_read_argument_stays_after_the_tail_call() {
    // The over-free face of the same reading. An inline `%`-opcode mints no region
    // of its own, so `(%first v)` hands the callee a borrow living IN `v`'s region —
    // which is why Rule 4 extends `v`'s own release to the reader, landing it in the
    // dead block. The descent passes THROUGH the opcode to `v`, so `v` stays exempt;
    // hoisting its release would free the pair the callee is handed.
    let module = compile_to_lir(
        "(begin (def p (fn (s) (%add 1 (length s)))) \
         (def q (fn (n) (let [v (%pair (string \"ab\" n) nil)] (p (%first v))))) \
         (q 1))",
    );
    // Named by the pair's OWN slot: the block legitimately releases other regions
    // ahead of the call (the materialized string's), so "some release precedes it"
    // says nothing about which.
    let (at, releases) = tail_call_slot_release_layout(&module, |i| match i {
        InstrRef::List { region, .. } => Some(*region),
        _ => None,
    })
    .expect("the body lowers to a TailCall over a cons cell");
    assert!(
        releases.iter().all(|&r| r > at),
        "the container of an opcode read's borrow was hoisted ahead of the \
         TailCall (at={at}, releases={releases:?}) — the moved value lives in it",
    );
}

/// Every `TailCall`'s `defer_callee_release` flag across the module, in emission
/// order. Reading the flag rather than a release position is what makes the
/// deferral pins specific: the release this channel supplies is emitted by the
/// RUNTIME at the callee's completion, so no instruction in the caller records it.
fn tail_call_deferrals(module: &FrozenModule) -> Vec<bool> {
    functions(module)
        .flat_map(flat_instrs)
        .filter_map(|i| match i {
            InstrRef::TailCall {
                defer_callee_release,
                ..
            } => Some(defer_callee_release),
            _ => None,
        })
        .collect()
}

#[test]
fn a_letrec_member_the_body_tail_calls_defers_its_own_release() {
    // `helper` is captured by `go`, so it is allocated per call and its uses span
    // the whole letrec — which puts its demise at the letrec's SCOPE END, not at
    // the call node the dies-here reading looks at. The relocation must leave that
    // release alone (the call is about to enter the closure it would free), so the
    // exemption's premise that the new activation takes it over holds only if this
    // channel runs it (docs/impl/region/mechanism.md § "What the exemption keeps, a
    // channel must still run"; the `tail-frame-exit-callee-member` probe).
    //
    // EXACTLY one deferral is the other half of the pin. `go`'s own body tail-calls
    // `helper` too, and a second deferral there would drop the frame's one
    // reference twice — which the marking's placement after the inits and the
    // non-upvalue guard each rule out on their own.
    let module = compile_to_lir(
        "(begin (def f (fn (n) \
         (letrec [helper (fn (x) (%sub x 1)) \
                   go (fn (m) (helper m))] \
           (helper (go n))))) (f 3))",
    );
    let deferrals = tail_call_deferrals(&module);
    assert_eq!(
        deferrals.iter().filter(|d| **d).count(),
        1,
        "the letrec member the body tail-calls must defer its release exactly \
         once (deferrals={deferrals:?}) — none strands one closure per call, two \
         drop the frame's single reference twice",
    );
}

/// A cell-free self-recursive callee keeps the deferral through every way its
/// letrec body can reach the tail call, and through a crossing of any frontier
/// (docs/impl/selfrec.md § "The deferral needs no escape gate" and § the placement
/// table). The channel is the region's only one — the scope-end `DecrefRegion` is
/// dead past the frame replacement — so a refusal here is one stranded closure and
/// env per call, which no release-position pin can see.
///
/// Four bodies, each varying one thing the predicate must NOT read: the plain tail
/// call, a statement before it (which ANF wraps so the body is no longer wholly a
/// tail call), one branch arm taking it, and the closure handed across the fiber
/// frontier before it. Each must defer exactly once — the body's tail call. `go`'s
/// own self-call is lowered with the init, before the marking, so it never adds a
/// second deferral that would drop the frame's single reference twice.
#[test]
fn a_stranded_self_recursive_callee_defers_through_every_body_shape() {
    for (label, body) in [
        ("a plain tail call", "(go n)"),
        (
            "a statement before the tail call",
            "(begin (%not n) (go n))",
        ),
        ("one branch arm", "(if n go (go n))"),
        (
            "a fiber crossing before the tail call",
            "(begin (emit 2 go) (go n))",
        ),
    ] {
        let module = compile_to_lir(&format!(
            "(begin (def f (fn (n) \
               (letrec [go (fn (m) (if m 0 (go true)))] {body}))) (f false))"
        ));
        let deferrals = tail_call_deferrals(&module);
        assert_eq!(
            deferrals.iter().filter(|d| **d).count(),
            1,
            "a letrec body reaching its self-recursive member through {label} must \
             defer that member's release exactly once (deferrals={deferrals:?})",
        );
    }
}

// ── What the fall-through owes, a signal exit owes too ───────────
// The post-`TailCall` block consumes the borrowed-argument retains a native
// callee never took over. That block runs on ONE outcome — the native's normal
// completion — so a signal exit needs the retains named on the instruction to
// consume them itself (docs/impl/region/mechanism.md § "What the fall-through
// owes, a signal exit owes too"). These pin the naming: the runtime can only
// consume what the lowerer recorded.

/// Every `TailCall` in the module, as its `borrowed_arg_slots` list.
fn tail_call_borrowed_slots(module: &FrozenModule) -> Vec<Vec<u16>> {
    functions(module)
        .flat_map(flat_instrs)
        .filter_map(|i| match i {
            InstrRef::TailCall {
                borrowed_arg_slots, ..
            } => Some(borrowed_arg_slots.iter().collect()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_borrowed_tail_argument_is_named_on_the_call() {
    // `x` is an upvalue of `g`, so `g`'s tail call to the native `length` mints a
    // fresh owning reference for the move and stashes the retained value in a
    // local. That stash is the only name a signal exit has for the retain.
    let module =
        compile_to_lir("(begin (def f (fn (x) (let [g (fn () (length x))] (g)))) (f (list 1 2)))");
    let slots = tail_call_borrowed_slots(&module);
    assert!(
        slots.iter().any(|s| s.len() == 1),
        "no call named its borrowed tail argument (slots={slots:?}) — a signal \
         exit can consume only what the instruction names",
    );
}

#[test]
fn an_owned_tail_argument_is_not_named_on_the_call() {
    // The over-free face: an OWNED argument's release IS the ownership move, and
    // on a signal exit the payload may be that very value — a fiber carrier hands
    // over its own fiber argument. Only the frame's EXTRA retain has a count
    // argument for being consumed early, so an owned argument must not be named.
    let module = compile_to_lir("(begin (def f (fn () (length (list 1 2)))) (f))");
    let slots = tail_call_borrowed_slots(&module);
    assert!(
        slots.iter().all(|s| s.is_empty()),
        "an owned tail argument was named as a borrowed retain (slots={slots:?}) — \
         releasing it at a signal exit drops the reference the move handed over",
    );
}

#[test]
fn a_tail_dynamic_emit_names_its_payload_as_a_borrowed_argument() {
    // A dynamic `emit` in tail position is an ordinary native tail call, and its
    // borrowed payload takes the ordinary borrowed-argument retain. That retain is
    // the body reference the park owes (docs/impl/region/owner.md § "What yields is
    // the emit OPERATION, not the `Emit` node"), so no second mint is owed here —
    // and the suspending exit's payload exemption has a slot to leave standing.
    let module =
        compile_to_lir("(let [s :yield] (fn () (let [x (string \"a\")] (fn () (emit s x)))))");
    let slots = tail_call_borrowed_slots(&module);
    assert!(
        slots.iter().any(|s| !s.is_empty()),
        "a tail dynamic emit must name its borrowed payload on the call \
         (slots={slots:?}) — the suspending exit can spare only what the \
         instruction names",
    );
}
