//! Primitive signal dispatch.
//!
//! Routes signal bits returned by NativeFn primitives to the appropriate
//! handler: stack push for SIG_OK, error storage for SIG_ERROR, fiber
//! execution for SIG_RESUME/SIG_PROPAGATE/SIG_CANCEL, VM state reads
//! for SIG_QUERY.

use crate::value::error_val;
use crate::value::{
    SignalBits, Value, SIG_CANCEL, SIG_ERROR, SIG_OK, SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
};
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
            SIG_QUERY => {
                // Primitive needs to read VM state. Value is a cons
                // cell (operation . argument) where operation is a
                // keyword or string.
                let (sig, result) = self.dispatch_query(value);
                if sig == SIG_ERROR {
                    self.fiber.signal = Some((SIG_ERROR, result));
                    self.fiber.stack.push(Value::NIL);
                } else {
                    self.fiber.stack.push(result);
                }
                None
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
            SIG_RESUME => self.handle_fiber_resume_signal_tail(value),
            SIG_PROPAGATE => self.handle_fiber_propagate_signal_tail(value),
            SIG_CANCEL => self.handle_fiber_cancel_signal_tail(value),
            SIG_QUERY => {
                let (sig, result) = self.dispatch_query(value);
                self.fiber.signal = Some((sig, result));
                sig
            }
            _ => {
                self.fiber.signal = Some((bits, value));
                bits
            }
        }
    }
}

impl VM {
    /// Dispatch a VM state query. Value is (operation . argument).
    ///
    /// The operation can be a keyword or a string. Keywords are resolved
    /// via the content-addressed keyword registry; strings are used
    /// directly. SIG_QUERY is for questions that can only be answered
    /// from the VM's context (call counts, global bindings, current fiber).
    ///
    /// Operations:
    /// - (:"call-count" . closure) — return call count for closure
    /// - (:"global?" . symbol) — return #t if symbol is bound as a global
    /// - (:"fiber/self" . _) — return the currently executing fiber, or nil
    fn dispatch_query(&self, value: Value) -> (SignalBits, Value) {
        let cons = match value.as_cons() {
            Some(c) => c,
            None => {
                return (
                    SIG_ERROR,
                    error_val("error", "SIG_QUERY: expected cons cell".to_string()),
                );
            }
        };

        // Accept keyword or string as operation identifier.
        let op_name: String = if let Some(name) = cons.first.as_keyword_name() {
            name.to_string()
        } else if let Some(s) = cons.first.as_string() {
            s.to_string()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    "SIG_QUERY: operation must be a keyword or string".to_string(),
                ),
            );
        };
        let arg = cons.rest;

        match op_name.as_str() {
            "call-count" => {
                if let Some(closure) = arg.as_closure() {
                    let ptr = closure.bytecode.as_ptr();
                    (SIG_OK, Value::int(self.get_closure_call_count(ptr) as i64))
                } else {
                    (SIG_OK, Value::int(0))
                }
            }
            "global?" => {
                if let Some(sym_id) = arg.as_symbol() {
                    (SIG_OK, Value::bool(self.get_global(sym_id).is_some()))
                } else {
                    (SIG_OK, Value::FALSE)
                }
            }
            "fiber/self" => (SIG_OK, self.current_fiber_value.unwrap_or(Value::NIL)),
            _ => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("SIG_QUERY: unknown operation: {}", op_name),
                ),
            ),
        }
    }
}
