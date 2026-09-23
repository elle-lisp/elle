// audited: 2026-09-23
//! Every example in the hand-written doc table runs as a program.
//!
//! docs/stdlib.md
//!
//! `(doc name)` prints these examples to a reader who will paste them, and
//! nothing else executes them, so a table entry can describe a macro that
//! changed shape long ago. The counter-factual: the table showed
//! `(defer (close handle))` and `(each (x (list 1 2 3)) (display x))`, neither
//! of which compiles, and every test stayed green.

use crate::pipeline::eval_all;
use crate::runtime::Runtime;

#[test]
fn every_hand_written_example_runs() {
    for doc in super::HAND_WRITTEN {
        if cfg!(not(feature = "ffi")) && doc.name.starts_with("ffi/") {
            continue;
        }
        let mut rt = Runtime::new();
        let (vm, symbols, cctx) = rt.parts();
        let result = eval_all(doc.example, symbols, vm, cctx, "<doc example>");
        assert!(
            result.is_ok(),
            "the example for `{}` does not run: {:?}\n{}",
            doc.name,
            result.err(),
            doc.example
        );
    }
}
