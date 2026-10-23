// audited: 2026-10-22
//! What `elle_jit_kill` leaves: the terminal `:killed` error as the fiber's
//! signal, `nil` as the function's result, and no parked frame.
//!
//! docs/impl/jit-fuel.md

use super::*;
use crate::value::fiber::{SIG_ERROR, SIG_TERMINAL};
use crate::value::heap::TableKey;

/// The `:error` kind of the struct `err`, as a keyword name match.
fn error_kind_is(err: Value, kind: &str) -> bool {
    err.as_struct()
        .expect("an error struct")
        .iter()
        .any(|(key, value)| {
            matches!(key, TableKey::Keyword(hash)
                if *hash == crate::value::keyword::keyword_hash("error"))
                && value.is_keyword_named(kind)
        })
}

#[test]
fn a_kill_sets_the_terminal_error_and_parks_nothing() {
    // Counter-factual: a kill raised as a plain :error is caught by the first
    // protect in the worker, which runs on; a kill that parks a frame reads a
    // charge site a silent function never recorded.
    crate::value::arena::with_test_region(|| {
        let mut vm = crate::vm::VM::new();
        let result = elle_jit_kill(&mut vm as *mut crate::vm::VM as *mut () as u64);
        assert_eq!(result, JitValue::nil(), "the function returns nil");
        let (sig, err) = vm.fiber.signal.expect("the kill sets the signal");
        assert_eq!(
            sig,
            SIG_ERROR.union(SIG_TERMINAL),
            "a kill is a terminal error"
        );
        assert!(error_kind_is(err, "killed"), "the error's kind is :killed");
        assert!(vm.fiber.suspended.is_none(), "a kill parks no frame");
    });
}
