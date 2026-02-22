//! Primitive signal dispatch.
//!
//! Routes signal bits returned by NativeFn primitives to the appropriate
//! handler: stack push for SIG_OK, error storage for SIG_ERROR, fiber
//! execution for SIG_RESUME/SIG_PROPAGATE/SIG_CANCEL.

use crate::value::{SignalBits, Value, SIG_CANCEL, SIG_ERROR, SIG_OK, SIG_PROPAGATE, SIG_RESUME};
use std::rc::Rc;

use super::core::VM;

impl VM {
    /// Handle signal bits returned by a primitive in a Call position.
    ///
    /// Returns `None` to continue the dispatch loop, or `Some(bits)` to
    /// return from the dispatch loop (for yields/signals).
    pub(super) fn handle_primitive_signal(
        &mut self,
        bits: SignalBits,
        value: Value,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        match bits {
            SIG_OK => {
                self.fiber.stack.push(value);
                None
            }
            SIG_ERROR => {
                // Store the error in fiber.signal. The dispatch loop will
                // see it and return SIG_ERROR.
                self.fiber.signal = Some((SIG_ERROR, value));
                self.fiber.stack.push(Value::NIL);
                None
            }
            SIG_RESUME => {
                // Primitive returned SIG_RESUME — dispatch to fiber handler
                self.handle_fiber_resume_signal(value, bytecode, constants, closure_env, ip)
            }
            SIG_PROPAGATE => {
                // fiber/propagate: re-raise the child fiber's signal
                self.handle_fiber_propagate_signal(value)
            }
            SIG_CANCEL => {
                // fiber/cancel: inject error into suspended fiber
                self.handle_fiber_cancel_signal(value, bytecode, constants, closure_env, ip)
            }
            _ => {
                // Any other signal (SIG_YIELD, user-defined)
                self.fiber.signal = Some((bits, value));
                Some(bits)
            }
        }
    }

    /// Handle signal bits returned by a primitive in a TailCall position.
    ///
    /// Always returns SignalBits (tail calls always return from the dispatch loop).
    pub(super) fn handle_primitive_signal_tail(
        &mut self,
        bits: SignalBits,
        value: Value,
    ) -> SignalBits {
        match bits {
            SIG_OK => {
                self.fiber.signal = Some((SIG_OK, value));
                SIG_OK
            }
            SIG_ERROR => {
                self.fiber.signal = Some((SIG_ERROR, value));
                SIG_ERROR
            }
            SIG_RESUME => {
                // Primitive in tail position — dispatch to fiber handler
                self.handle_fiber_resume_signal_tail(value)
            }
            SIG_PROPAGATE => {
                // fiber/propagate in tail position
                self.handle_fiber_propagate_signal_tail(value)
            }
            SIG_CANCEL => {
                // fiber/cancel in tail position
                self.handle_fiber_cancel_signal_tail(value)
            }
            _ => {
                self.fiber.signal = Some((bits, value));
                bits
            }
        }
    }
}
