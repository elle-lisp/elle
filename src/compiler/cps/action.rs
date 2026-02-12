//! Action enum - results from CPS code execution

use super::Continuation;
use crate::value::Value;
use std::rc::Rc;

/// Result of executing CPS-transformed code
///
/// Instead of returning values directly, CPS code returns Actions
/// that tell the trampoline what to do next.
#[derive(Debug, Clone)]
pub enum Action {
    /// Return a value and continue with the current continuation
    Return {
        value: Value,
        continuation: Rc<Continuation>,
    },

    /// Yield a value and suspend execution (for coroutines)
    /// The continuation is saved so execution can resume later
    Yield {
        /// Value to yield to the caller
        value: Value,
        /// Saved continuation for resumption
        continuation: Rc<Continuation>,
    },

    /// Perform a function call
    Call {
        /// Function to call
        func: Value,
        /// Arguments to pass
        args: Vec<Value>,
        /// Continuation after call returns
        continuation: Rc<Continuation>,
    },

    /// Perform a tail call (no continuation needed)
    TailCall {
        /// Function to call
        func: Value,
        /// Arguments to pass
        args: Vec<Value>,
    },

    /// Execution completed with final value
    Done(Value),

    /// Execution failed with error
    Error(String),
}

impl Action {
    /// Create a return action
    pub fn return_value(value: Value, continuation: Rc<Continuation>) -> Self {
        if continuation.is_done() {
            Action::Done(value)
        } else {
            Action::Return {
                value,
                continuation,
            }
        }
    }

    /// Create a yield action
    pub fn yield_value(value: Value, continuation: Rc<Continuation>) -> Self {
        Action::Yield {
            value,
            continuation,
        }
    }

    /// Create a call action
    pub fn call(func: Value, args: Vec<Value>, continuation: Rc<Continuation>) -> Self {
        Action::Call {
            func,
            args,
            continuation,
        }
    }

    /// Create a tail call action
    pub fn tail_call(func: Value, args: Vec<Value>) -> Self {
        Action::TailCall { func, args }
    }

    /// Create a done action
    pub fn done(value: Value) -> Self {
        Action::Done(value)
    }

    /// Create an error action
    pub fn error(msg: impl Into<String>) -> Self {
        Action::Error(msg.into())
    }

    /// Check if this action is done
    pub fn is_done(&self) -> bool {
        matches!(self, Action::Done(_))
    }

    /// Check if this action is a yield
    pub fn is_yield(&self) -> bool {
        matches!(self, Action::Yield { .. })
    }

    /// Check if this action is an error
    pub fn is_error(&self) -> bool {
        matches!(self, Action::Error(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_return_with_done_continuation() {
        let cont = Continuation::done();
        let action = Action::return_value(Value::Int(42), cont);
        assert!(action.is_done());
    }

    #[test]
    fn test_yield_action() {
        let cont = Continuation::done();
        let action = Action::yield_value(Value::Int(1), cont);
        assert!(action.is_yield());
    }

    #[test]
    fn test_error_action() {
        let action = Action::error("test error");
        assert!(action.is_error());
    }
}
