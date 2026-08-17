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
fn tail_call_release_layout(module: &crate::lir::LirModule) -> Option<(usize, Vec<usize>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefValueRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
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
fn tail_call_region_release_layout(module: &crate::lir::LirModule) -> Option<(usize, Vec<usize>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
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
        LirInstr::List { region, .. } => Some(*region),
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
fn tail_call_deferrals(module: &crate::lir::LirModule) -> Vec<bool> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    funcs
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .filter_map(|i| match &i.instr {
            LirInstr::TailCall {
                defer_callee_release,
                ..
            } => Some(*defer_callee_release),
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

/// Position of the first `TailCall` in the function that contains one, with the
/// indices of that block's `DecrefRegion`s naming the region `of` picks out of the
/// same block's allocating instructions.
///
/// Reading by REGION rather than by instruction count is what makes a decline pin
/// specific: a block releases several regions around its tail call, so "some
/// release precedes it" says nothing about which one did.
fn tail_call_slot_release_layout(
    module: &crate::lir::LirModule,
    of: impl Fn(&LirInstr) -> Option<StaticRegion>,
) -> Option<(usize, Vec<usize>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            let Some(want) = b.instructions.iter().find_map(|i| of(&i.instr)) else {
                continue;
            };
            let releases = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| {
                    matches!(i.instr, LirInstr::DecrefRegion { region_id } if region_id == want)
                })
                .map(|(idx, _)| idx)
                .collect();
            return Some((at, releases));
        }
    }
    None
}

/// Position of the first `TailCall` in the first block that has one AND releases
/// a cell there, with the indices of that block's `DecrefCellRegion`s — the
/// env-cell twin of [`tail_call_release_layout`], which reads the value route.
///
/// A block with no cell release is skipped rather than returned: its empty
/// release list would satisfy a placement assertion in either direction, so
/// returning it would let a pin pass while measuring nothing.
fn tail_call_cell_release_layout(module: &crate::lir::LirModule) -> Option<(usize, Vec<usize>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            let releases: Vec<usize> = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefCellRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            if releases.is_empty() {
                continue;
            }
            return Some((at, releases));
        }
    }
    None
}

/// For each block that makes no `TailCall` and does release a cell, the index of
/// its last `LoadCapture` (the arm's read through the cell, `None` when it makes
/// none) beside the indices of its `DecrefCellRegion`s.
///
/// A `tail`-route release must land AFTER the arm's read; a `head`-route one lands
/// at the arm's head, before any read there is.
fn fallthrough_cell_read_and_release(
    module: &crate::lir::LirModule,
) -> Vec<(Option<usize>, Vec<usize>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    funcs
        .flat_map(|f| f.blocks.iter())
        .filter(|b| {
            !b.instructions
                .iter()
                .any(|i| matches!(i.instr, LirInstr::TailCall { .. }))
        })
        .filter_map(|b| {
            let releases: Vec<usize> = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::DecrefCellRegion { .. }))
                .map(|(idx, _)| idx)
                .collect();
            if releases.is_empty() {
                return None;
            }
            let read = b
                .instructions
                .iter()
                .enumerate()
                .filter(|(_, i)| matches!(i.instr, LirInstr::LoadCapture { .. }))
                .map(|(idx, _)| idx)
                .next_back();
            Some((read, releases))
        })
        .collect()
}

/// The `DecrefCellRegion` counts of the blocks that make no `TailCall` at all —
/// where a branch arm's head compensation lands when its sibling leaves through a
/// callee.
fn cell_releases_in_fallthrough_blocks(module: &crate::lir::LirModule) -> Vec<usize> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    funcs
        .flat_map(|f| f.blocks.iter())
        .filter(|b| {
            !b.instructions
                .iter()
                .any(|i| matches!(i.instr, LirInstr::TailCall { .. }))
        })
        .map(|b| {
            b.instructions
                .iter()
                .filter(|i| matches!(i.instr, LirInstr::DecrefCellRegion { .. }))
                .count()
        })
        .collect()
}

#[test]
fn reassigned_env_cell_release_precedes_the_frame_replacing_tail_call() {
    // `c` is a captured local, so `populate_env` mints its cell box once per
    // activation and the box's `DecrefCellRegion` lands in the dead block. It is
    // hoisted even though `c` is REASSIGNED: the mutated refusal is compensation's
    // release-ROUTE one, and this release names the box (`LoadCaptureRaw`), which
    // an `assign` never repoints — it writes the cell's content
    // (docs/impl/region/mechanism.md § "A mutated holder poisons its value route,
    // not its cell box"; the `fresh-env-cell` probe).
    let module = compile_to_lir(
        "(begin (def f (fn () (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (g)))) (f))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the reassigned env cell's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path, one box \
         stranded per activation",
    );
}

#[test]
fn escaping_holder_env_cell_release_stays_after_the_tail_call() {
    // The decline face: the closure holding the cell crosses the FIBER frontier
    // before the body tail-calls it, so escape's capture facet marks `c` escaping
    // beyond return and both admissions refuse the box — a resumer holds the
    // closure through a hold the compiler did not place. Only the mutated refusal
    // is scoped to the value route; an escape facet no edge at the point replaces
    // still refuses, and the release keeps its place in the dead block.
    let module = compile_to_lir(
        "(begin (def f (fn () (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (begin (emit :yield g) (g))))) \
         (fiber/new f |:yield|))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r > at),
        "an escaping holder's env cell was hoisted ahead of the TailCall \
         (at={at}, releases={releases:?}) — the closure leaves carrying the cell",
    );
}

#[test]
fn a_falling_through_arm_head_releases_the_env_cell_its_sibling_relocated() {
    // `(if t (g) 0)` — the box's one `DecrefCellRegion` relocates into the arm that
    // tail-calls `g`, so the arm that falls through to the merge would release
    // nothing. That arm names `c` nowhere, so branch compensation's head route
    // covers it; the two are mutually exclusive by arm structure, which is what a
    // cell release needs because it leaves no nil-stamp to make a replica no-op
    // (docs/impl/region/mechanism.md § "A compensating release of an env cell names
    // the box, not the holder's slot").
    let module = compile_to_lir(
        "(begin (def f (fn (t) (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (if t (g) 0)))) (f false))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r < at),
        "the tail-calling arm must keep its relocated cell release ahead of the \
         TailCall (at={at}, releases={releases:?})",
    );
    let fallthrough = cell_releases_in_fallthrough_blocks(&module);
    assert!(
        fallthrough.contains(&1),
        "some block that makes no tail call must release the cell exactly once — \
         the falling-through arm's head compensation; per-block counts={fallthrough:?}",
    );
    assert!(
        fallthrough.iter().all(|&n| n <= 1),
        "no block may release the cell twice; per-block counts={fallthrough:?}",
    );
}

#[test]
fn a_reading_arm_tail_releases_the_env_cell_its_sibling_relocated() {
    // `(if t c (g))` — the same relocation, and the sibling arm READS `c`. The
    // capture-use of `c` resolves through `g`'s last use, so the `decref_point`
    // follows the call rather than the read and lands in the LATER arm. A head
    // release on the reading arm would free the box under that read, so the arm
    // takes the `tail` route instead: one `DecrefCellRegion` after its
    // `LoadCapture`. The route's same-node retain is a claim about the value the
    // holder names; the box's own holders are the frame's env slot and the
    // capturer's counted edge (docs/impl/region/mechanism.md § "A compensating
    // release of an env cell names the box, not the holder's slot").
    let module = compile_to_lir(
        "(begin (def f (fn (n t) (def @c n) \
         (let [g (fn () c)] (if t c (g))))) (f 1 true))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r < at),
        "the tail-calling arm must keep its relocated cell release ahead of the \
         TailCall (at={at}, releases={releases:?})",
    );
    let arms = fallthrough_cell_read_and_release(&module);
    assert!(
        arms.iter()
            .any(|(read, rel)| read.is_some_and(|r| rel.iter().all(|&d| d > r)) && rel.len() == 1),
        "the reading arm must release the box exactly once, after its own read \
         through the cell; per-block (last LoadCapture, DecrefCellRegion)={arms:?}",
    );
    assert!(
        arms.iter().all(|(_, rel)| rel.len() <= 1),
        "no block may release the cell twice; per-block reads/releases={arms:?}",
    );
}

/// For the first function with two `TailCall`-bearing blocks — a branch whose
/// arms each make one — the local slots each block releases BEFORE its call and
/// those it releases after.
///
/// Reading by SLOT rather than by instruction count is what makes these pins
/// specific: an arm carries the replicated release of *every* region the merge
/// releases, so "some release precedes the call" says nothing about which.
fn branch_arm_release_slots(module: &crate::lir::LirModule) -> Vec<(Vec<u16>, Vec<u16>)> {
    let funcs = std::iter::once(&module.entry).chain(module.closures.iter());
    for f in funcs {
        let mut arms = Vec::new();
        for b in &f.blocks {
            let Some(at) = b
                .instructions
                .iter()
                .position(|i| matches!(i.instr, LirInstr::TailCall { .. }))
            else {
                continue;
            };
            let mut from_slot: std::collections::HashMap<Reg, u16> =
                std::collections::HashMap::new();
            let (mut before, mut after) = (Vec::new(), Vec::new());
            for (idx, i) in b.instructions.iter().enumerate() {
                match &i.instr {
                    LirInstr::LoadLocal { dst, slot } => {
                        from_slot.insert(*dst, *slot);
                    }
                    LirInstr::DecrefValueRegion { src } => {
                        if let Some(&slot) = from_slot.get(src) {
                            if idx < at {
                                before.push(slot);
                            } else {
                                after.push(slot);
                            }
                        }
                    }
                    _ => {}
                }
            }
            arms.push((before, after));
        }
        if arms.len() >= 2 {
            return arms;
        }
    }
    Vec::new()
}

#[test]
fn stranded_param_release_is_replicated_into_every_branch_arm() {
    // The release lands past the MERGE, which each arm leaves through a
    // frame-replacing tail call — so the merge copy alone reaches neither path.
    // The merge's inherited relocation points put a copy ahead of each arm's
    // `TailCall` (docs/impl/region/mechanism.md § "The relocation point outlives
    // the block"; the `tail-frame-exit-arms` probe). `x` is the first parameter,
    // hence local slot 0.
    let module = compile_to_lir(
        "(begin (def s (fn () 0)) (def s2 (fn () 1)) \
         (def f (fn (x t) (if t (s) (s2)))) (f (list 1 2) true))",
    );
    let arms = branch_arm_release_slots(&module);
    assert_eq!(arms.len(), 2, "the body lowers to one TailCall per arm");
    for (before, after) in &arms {
        assert!(
            before.contains(&0),
            "an arm's copy of the stranded parameter's release is missing \
             (before={before:?}, after={after:?}) — dead on that arm's closure path",
        );
    }
}

#[test]
fn moved_argument_takes_no_replica_in_the_arm_that_moves_it() {
    // The exemption is read PER point. `x` (local slot 0) is the then-arm call's
    // argument, so that arm takes no replica of `x`'s release — the callee's
    // owned-parameter release is what frees it there. The same arm still takes a
    // replica of `t`'s release, and the merge's other point, whose call names
    // nothing, takes one of `x`'s.
    let module = compile_to_lir(
        "(begin (def s (fn (a) a)) (def s2 (fn () 1)) \
         (def f (fn (x t) (if t (s x) (s2)))) (f (list 1 2) true))",
    );
    let arms = branch_arm_release_slots(&module);
    assert_eq!(arms.len(), 2, "the body lowers to one TailCall per arm");
    let (moving_before, moving_after) = &arms[0];
    assert!(
        !moving_before.contains(&0),
        "the moved argument's release was replicated ahead of the arm's TailCall \
         (before={moving_before:?}) — that release IS the ownership move",
    );
    assert!(
        !moving_after.contains(&0),
        "the moved argument's release was left in the arm's dead block \
         (after={moving_after:?}) — nothing there runs",
    );
    assert!(
        moving_before.contains(&1),
        "the arm took no replica at all (before={moving_before:?}) — the exemption \
         is read per REGION at each point, not per point",
    );
    let (other_before, _) = &arms[1];
    assert!(
        other_before.contains(&0),
        "the sibling arm, whose call names nothing, did not take the replica \
         (before={other_before:?})",
    );
}
