// audited: 2026-11-22
//! Each register a lowered function writes, it writes once, and each register it reads, some instruction writes.
//!
//! src/lir/overview.md
//! docs/spec/form.md

use super::*;

/// The registers `func` writes, once per write. A `TailCall`'s `dst` counts:
/// the native fall-through writes it, and the block after the call reads it.
fn writes(func: &LirOwned) -> Vec<Reg> {
    func.view()
        .nodes()
        .filter_map(|n| match n.instr() {
            InstrRef::TailCall { dst, .. } => Some(dst),
            _ => n.def(),
        })
        .collect()
}

/// Each register some function of `module` writes more than once, as
/// (function index, register), the entry function first.
fn rewritten(module: &FrozenModule) -> Vec<(usize, Reg)> {
    let mut out = Vec::new();
    for (index, func) in functions(module).enumerate() {
        let mut seen = std::collections::HashSet::new();
        for reg in writes(func) {
            if !seen.insert(reg) {
                out.push((index, reg));
            }
        }
    }
    out
}

/// Each register some function of `module` reads and never writes, as
/// (function index, register), the entry function first.
fn unwritten(module: &FrozenModule) -> Vec<(usize, Reg)> {
    let mut out = Vec::new();
    for (index, func) in functions(module).enumerate() {
        let written: std::collections::HashSet<Reg> = writes(func).into_iter().collect();
        let mut read = Vec::new();
        for block in func.view().blocks() {
            for node in block.nodes() {
                read.extend_from_slice(node.uses());
            }
            crate::lir::for_each_terminator_use(&block.terminator(), |r| read.push(r));
        }
        for reg in read {
            if !written.contains(&reg) && !out.contains(&(index, reg)) {
                out.push((index, reg));
            }
        }
    }
    out
}

/// Whether some instruction of `module` is followed directly by one that
/// satisfies `then`, where it satisfies `first`.
fn has_pair(
    module: &FrozenModule,
    first: impl Fn(&InstrRef<'_>) -> bool,
    then: impl Fn(&InstrRef<'_>) -> bool,
) -> bool {
    functions(module).any(|f| {
        flat_instrs(f)
            .windows(2)
            .any(|w| first(&w[0]) && then(&w[1]))
    })
}

#[test]
fn an_unslotted_result_release_reads_its_value_back_into_a_fresh_register() {
    // A call result no slot names is released by value off a scratch slot, and
    // the node's value is read back from that slot. Counterfactual: reading it
    // back into the call's own register writes that register twice.
    let module = compile_to_lir("(let [a (@array 1 2)] (fn [] (length a)))");
    assert!(
        has_pair(
            &module,
            |i| matches!(i, InstrRef::DecrefValueRegion { .. }),
            |i| matches!(i, InstrRef::LoadLocal { .. })
        ),
        "the source must reach the unslotted release, which reads its value back"
    );
    assert_eq!(rewritten(&module), vec![], "no register is written twice");
}

#[test]
fn a_closure_adopt_reads_the_closure_back_into_a_fresh_register() {
    // A closure that adopts its capture is spilled to a scratch slot for the
    // adopt and read back after it. Counterfactual: reading it back into the
    // `MakeClosure`'s own register writes that register twice.
    let module =
        compile_to_lir("(begin (let [p (%pair 1 2)] (let [c (fn [] (length p))] (c))) nil)");
    assert!(
        count_adopt_regions(&module) >= 1,
        "the source must reach the closure-construction adopt"
    );
    assert_eq!(rewritten(&module), vec![], "no register is written twice");
}

#[test]
fn an_emit_payload_retain_reads_the_payload_back_into_a_fresh_register() {
    // A suspending emit of a payload the body releases nowhere, here a captured
    // parameter, retains it and spills it to a slot of its own, and the emit
    // reads it back. Counterfactual: reading it back into the payload's own
    // register writes that register twice.
    let module = compile_to_lir("(fn (p) (fn () (emit :yield p)))");
    let spilled = functions(&module).any(|f| {
        f.view().blocks().any(|b| {
            let instrs: Vec<_> = b.nodes().map(|n| n.instr()).collect();
            matches!(b.terminator(), Terminator::Emit { .. })
                && matches!(
                    instrs.as_slice(),
                    [
                        ..,
                        InstrRef::IncrefValueRegion { .. },
                        InstrRef::StoreLocal { .. },
                        InstrRef::LoadLocal { .. }
                    ]
                )
        })
    });
    assert!(
        spilled,
        "the source must reach the emit payload's retain and spill"
    );
    assert_eq!(rewritten(&module), vec![], "no register is written twice");
}

#[test]
fn a_break_out_of_an_arm_leaves_no_read_of_a_register_nothing_writes() {
    // The arm of an `if` that breaks out of its block never reaches the code
    // after it, and neither does one that breaks out of a `while`.
    // Counterfactual: the lowerer still emits the arm's join, a block no edge
    // reaches, which stores the arm's value from a register no instruction
    // writes.
    let unread: Vec<_> = [
        "(fn [c] (def r (block :b (def a (@array 1)) (if c (break :b a) nil) (@array 2))) (length r))",
        "(fn [c] (def @a (@array 1)) (while true (length a) (if c (assign a (@array 2)) (break))) nil)",
    ]
    .into_iter()
    .map(|source| (source, unwritten(&compile_to_lir(source))))
    .filter(|(_, regs)| !regs.is_empty())
    .collect();
    assert_eq!(unread, vec![], "every register read is written");
}
