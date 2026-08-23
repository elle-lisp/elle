//! Dynamic parameter-frame opcode bodies.
//!
//! Split out of the dispatch match because building a `parameterize` frame is
//! long enough to obscure the surrounding opcode routing. Behavior is
//! unchanged; the loop simply calls these methods.

use super::*;

impl VM {
    /// `PushParamFrame`: collect `count` (parameter, value) pairs off the stack
    /// and install them as a new dynamic-parameter frame.
    ///
    /// The stack holds pairs pushed as `[param1, val1, param2, val2, ...]`, so
    /// they pop in reverse (last pair first); we re-reverse to restore source
    /// order before installing. A non-parameter operand raises a type-error and
    /// aborts the frame (pushing nil for the aborted `parameterize` result);
    /// the error's unwind also skips the scope-end `PopParamFrame`, so push
    /// and pop stay balanced on both paths.
    ///
    /// The abort must be decided by THIS opcode's own failure, never by
    /// `fiber.signal`: that slot ambiently carries the `(SIG_OK, value)`
    /// return-value handoff between frames, so a signal can be pending while
    /// execution is entirely healthy. Gating the push on it skips a frame
    /// whose balanced pop still runs, and that pop then consumes the frame
    /// below — an enclosing `parameterize`'s, or the fiber's seeded parameter
    /// baseline (pinned by `tests/elle/param-frame-balance.lisp`).
    #[inline]
    pub(super) fn handle_push_param_frame(&mut self, bc: &[u8], ip: &mut usize) {
        let count = bc[*ip] as usize;
        *ip += 1;
        let mut frame = Vec::with_capacity(count);
        let mut raw_pairs = Vec::with_capacity(count);
        for _ in 0..count {
            let val = self
                .fiber
                .stack
                .pop()
                .expect("VM bug: stack underflow in PushParamFrame");
            let param = self
                .fiber
                .stack
                .pop()
                .expect("VM bug: stack underflow in PushParamFrame");
            raw_pairs.push((param, val));
        }
        for (param, val) in raw_pairs.into_iter().rev() {
            if let Some((id, _default)) = param.as_parameter() {
                frame.push((id, val));
            } else {
                self.set_error(
                    "type-error",
                    format!("parameterize: {} is not a parameter", param.type_name()),
                );
                self.fiber.stack.push(Value::NIL);
                return;
            }
        }
        self.fiber.param_frames.push(frame);
    }
}
