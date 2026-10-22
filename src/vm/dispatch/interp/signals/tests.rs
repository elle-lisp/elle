// audited: 2026-10-22
//! The panic a signal handler in the dispatch loop raises on stack underflow.
//!
//! src/vm/overview.md

use super::*;

// Stack underflow is a VM bug, and every handler panics on it
// (src/vm/overview.md). Counter-factual: a handler that reads NIL for a missing
// operand passes the bound check as a non-closure, and the broken bytecode
// runs on.
#[test]
#[should_panic(expected = "VM bug: Stack underflow on CheckSignalBound")]
fn check_signal_bound_panics_on_stack_underflow() {
    let mut vm = VM::new();
    assert!(
        vm.fiber.stack.is_empty(),
        "a fresh VM's operand stack is empty"
    );
    let allowed_bits = 0u64.to_be_bytes();
    let mut ip = 0;
    vm.handle_check_signal_bound(&allowed_bits, &mut ip);
}
