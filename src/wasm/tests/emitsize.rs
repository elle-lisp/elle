// audited: 2026-10-06
// docs/impl/wasm.md
//! What a function that may signal is allowed to cost: its spill and restore
//! code, and the edges through its dispatch loop.
//!
//! Such a function spills live state where it parks and restores it on resume.
//! The code for this must scale with the *live* set at each park, not with
//! `parks × total_slots`. When it does not, a large stdlib function that may
//! signal (for example, a string builder) emits a multi-megabyte function body
//! that exceeds Wasmtime's per-function size limit, and the whole module fails
//! to parse.

use super::*;

#[test]
fn resume_and_spill_are_linear_in_slots_not_states_times_slots() {
    // 80 yield points, 150 dead local slots. With per-state dense restore and
    // dense local spill the body is ~O(states × slots) ≈ hundreds of KB; with
    // hoisted restore and live-aware local spill it is ~O(states + slots).
    let bytes = emit_bytes(suspending_closure(80, 150));

    // Generous ceiling: the linear emission is a few KB of dispatch + one
    // restore of the declared slots. The quadratic emission is ~280KB. 40KB
    // sits well above the linear size and well below the quadratic one.
    assert!(
        bytes.len() < 40_000,
        "suspending closure emitted {} bytes — spill/restore is scaling with \
         states × slots instead of live slots (CPS size blowup)",
        bytes.len()
    );
}

#[test]
fn dead_locals_do_not_multiply_by_state_count() {
    // The core invariant: adding local slots that are dead across every suspend
    // point must add O(slots) to the module (they are declared and reloaded once
    // by the hoisted restore), never O(states × slots) (spilled AND restored at
    // every one of the suspend points). Hold the state count fixed and grow the
    // dead-local count by 500.
    let few = emit_bytes(suspending_closure(100, 20)).len();
    let many = emit_bytes(suspending_closure(100, 520)).len();
    // 500 extra dead locals across 100 states. Live-aware, hoisted emission adds
    // a few KB (one restore of the extra slots + their declarations); the
    // states × slots emission would add ~100 × 500 slots of spill + restore,
    // on the order of a megabyte.
    assert!(
        many - few < 60_000,
        "500 extra dead locals grew the module by {} bytes at 100 states — \
         spill/restore is scaling with states × slots, not live slots",
        many - few
    );
}

/// A closure that may signal, of one block that makes `n_calls` calls in a row.
/// Each call is a park site, so the block splits after every call but the last.
fn calling_closure(n_calls: u32) -> LirOwned {
    let regs: Vec<Reg> = (0..=n_calls).map(Reg).collect();
    let mut instrs = vec![InstrRef::Const {
        dst: Reg(0),
        value: ConstRef::Int(0),
    }];
    for i in 0..n_calls as usize {
        instrs.push(InstrRef::Call {
            dst: regs[i + 1],
            func: Reg(0),
            args: &regs[i..i + 1],
            arity_checked: false,
            region: static_region(1),
        });
    }
    LirFixture::new(Arity::Exact(0))
        .closure_id(ClosureId(0))
        .signal(Signal::errors())
        .block(0, &instrs, Terminator::Return(Reg(n_calls)))
        .build()
}

/// How many branches in the `n`-th function body of `bytes` target a `loop`.
fn loop_branches(bytes: &[u8], n: usize) -> usize {
    use wasmparser::{Operator, Parser, Payload};
    let body = Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.expect("the module parses") {
            Payload::CodeSectionEntry(body) => Some(body),
            _ => None,
        })
        .nth(n)
        .expect("the module has the function body");
    // One entry per open structured block: whether it is a loop.
    let mut frames: Vec<bool> = Vec::new();
    let mut count = 0;
    let mut ops = body.get_operators_reader().expect("the body has code");
    while !ops.eof() {
        let depths: Vec<u32> = match ops.read().expect("an operator") {
            Operator::Block { .. } | Operator::If { .. } => {
                frames.push(false);
                continue;
            }
            Operator::Loop { .. } => {
                frames.push(true);
                continue;
            }
            Operator::End => {
                frames.pop();
                continue;
            }
            Operator::Br { relative_depth } | Operator::BrIf { relative_depth } => {
                vec![relative_depth]
            }
            Operator::BrTable { targets } => targets
                .targets()
                .map(|t| t.expect("a br_table target"))
                .chain([targets.default()])
                .collect(),
            _ => continue,
        };
        count += depths
            .iter()
            .filter(|&&d| {
                frames
                    .len()
                    .checked_sub(1 + d as usize)
                    .is_some_and(|i| frames[i])
            })
            .count();
    }
    count
}

#[test]
fn a_park_site_reaches_the_code_after_it_without_the_loop() {
    // Wasmtime merges every value the dispatch loop carries once for each edge
    // into the loop header. A function that may signal parks at every call, so
    // its blocks split at every call. The counter-factual sends each split's
    // edge, and each resume entry's, round the loop: about two edges per call,
    // times every live value, which took Wasmtime minutes and gigabytes on a
    // function of a few hundred calls. Straight-line code has no edge back.
    let bytes = emit_bytes(calling_closure(50));
    assert_eq!(
        loop_branches(&bytes, 1),
        0,
        "a straight-line function that may signal must not branch round its \
         dispatch loop"
    );
}
