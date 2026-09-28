// audited: 2026-09-28
//! What the root entry does to the operand stack it finds: nothing.
//!
//! docs/impl/vm.md
//!
//! A body addresses local `n` at stack position `n`, so `execute_code` runs a
//! top-level body on a stack of its own and gives the one it found back. These
//! pins seed that stack with values the program does not own and run a
//! program the way `eval_file` does.

use super::*;
use crate::pipeline::compile_file;

/// Compile `src` as a file and run it at the root over `below`, the operand
/// stack the entry finds. Answers the program's value, the stack the entry
/// left, and the depth the body itself left at its exit.
fn run_over(
    rt: &mut Runtime,
    below: &[crate::value::Value],
    src: &str,
) -> (crate::value::Value, Vec<crate::value::Value>, usize) {
    let result = {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file(src, symbols, cctx, "<rootentry>").expect("compiles")
    };
    let (vm, _symbols, _cctx) = rt.parts();
    vm.fiber.stack.clear();
    vm.fiber.stack.extend(below.iter().copied());
    let value = vm.execute(&result.bytecode).expect("runs");
    let left = vm.fiber.stack.to_vec();
    (value, left, vm.root_exit_depth())
}

#[test]
fn a_root_body_gives_back_the_stack_it_found() {
    // Counter-factual: a body run above the values it found stores its locals
    // over the lowest of them and leaves its `Nil` prologue on top, so the stack
    // comes back longer and with its bottom rewritten.
    let mut rt = Runtime::new();
    let below: Vec<_> = (0..16)
        .map(|i| crate::value::Value::int(1000 + i))
        .collect();
    let (value, left, _) = run_over(&mut rt, &below, "(def a 1) (def b 2) (+ a b)");
    assert_eq!(value, crate::value::Value::int(3));
    assert_eq!(
        left, below,
        "the root entry must hand back exactly the operand stack it found"
    );
}

#[test]
fn a_root_body_never_releases_a_value_below_it() {
    // The taken branch never writes the slot that names `(f)`, and the join
    // releases that slot on every path. Run above a heap value, the slot is that
    // value, and the release frees its region under the holder.
    //
    // Counter-factual: the root entry runs the body above the seeded values, so
    // the sentinel's region drops to rc 0 while the stack below still holds it.
    let mut rt = Runtime::new();
    let (sentinel, region) = alloc_in_fresh_region(rt.heap(), cons());
    let below = vec![sentinel; 64];
    let (value, _, _) = run_over(
        &mut rt,
        &below,
        "(defn f [] @[1]) (if (integer? 1) nil (f)) true",
    );
    assert_eq!(value, crate::value::Value::TRUE);
    assert_eq!(
        region_rc(rt.heap(), region),
        1,
        "a value below the root body is not the body's to release"
    );
}

#[test]
fn the_root_exit_depth_counts_what_the_body_left() {
    // The operand-residue pins read this gauge, so it has to move: a body with
    // three top-level bindings reserves at least three local slots, and they
    // are still on its stack at its exit.
    //
    // Counter-factual: a gauge that reads zero lets every residue pin pass
    // whatever the loop leaves behind.
    let mut rt = Runtime::new();
    let (_, _, depth) = run_over(&mut rt, &[], "(def a 1) (def b 2) (def c 3) (+ a b c)");
    assert!(
        depth >= 3,
        "the body's three locals must count toward its exit depth (depth={depth})"
    );
}
