// audited: 2026-10-06
//! The parameter-frame helper keeps push and pop balanced when it raises.
//!
//! docs/parameters.md
//! docs/impl/jit.md

use super::*;

/// A non-parameter operand raises and still pushes a frame, one that binds
/// nothing, as the interpreter's `PushParamFrame` does.
///
/// The counter-factual: a helper that raises without pushing leaves the
/// `parameterize` body's scope-end pop to consume the frame below it, so a
/// restart past the raise loses an enclosing binding
/// (tests/impl/jit-error-park.lisp).
#[test]
fn a_raising_push_param_frame_pushes_an_empty_frame() {
    crate::value::arena::with_test_region(|| {
        let mut vm = vm_with_primitives();
        let below = vm.fiber.param_frames.len();
        let pairs = [Value::int(1), Value::int(2)];
        elle_jit_push_param_frame(pairs.as_ptr(), 1, vm_arg(&mut vm));
        assert!(
            matches!(vm.fiber.signal, Some((bits, _)) if bits.intersects(crate::value::SIG_ERROR)),
            "a non-parameter raises",
        );
        assert_eq!(
            vm.fiber.param_frames.len(),
            below + 1,
            "the raise pushes a frame for the scope-end pop",
        );
        assert!(
            vm.fiber.param_frames.last().is_some_and(|f| f.is_empty()),
            "the frame binds nothing",
        );
    });
}
