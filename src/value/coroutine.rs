//! Coroutine types for the Elle runtime
//!
//! Coroutines are suspendable computations that can yield values
//! and be resumed later.

use crate::value::closure::Closure;
use crate::value::Value;
use std::rc::Rc;

/// Coroutine execution state
#[derive(Debug, Clone)]
pub enum CoroutineState {
    /// Coroutine has not started
    Created,
    /// Coroutine is running
    Running,
    /// Coroutine is suspended (yielded)
    Suspended,
    /// Coroutine has completed
    Done,
    /// Coroutine encountered an error
    Error(String),
}

/// A coroutine value - a suspendable computation
#[derive(Debug, Clone)]
pub struct Coroutine {
    /// The coroutine's closure
    pub closure: Rc<Closure>,
    /// Current state
    pub state: CoroutineState,
    /// Last yielded value (if suspended)
    pub yielded_value: Option<Value>,
    /// Saved first-class continuation for yield across call boundaries.
    /// This is a Value containing ContinuationData.
    pub saved_value_continuation: Option<Value>,
}

impl Coroutine {
    /// Create a new coroutine from a closure
    pub fn new(closure: Rc<Closure>) -> Self {
        Coroutine {
            closure,
            state: CoroutineState::Created,
            yielded_value: None,
            saved_value_continuation: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::value::types::Arity;
    use std::collections::HashMap;

    #[test]
    fn test_coroutine_new() {
        let closure = Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
        });

        let co = Coroutine::new(closure);
        assert!(matches!(co.state, CoroutineState::Created));
        assert!(co.yielded_value.is_none());
        assert!(co.saved_value_continuation.is_none());
    }

    #[test]
    fn test_coroutine_state_transitions() {
        let closure = Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
        });

        let mut co = Coroutine::new(closure);

        // Transition to Running
        co.state = CoroutineState::Running;
        assert!(matches!(co.state, CoroutineState::Running));

        // Transition to Suspended
        co.state = CoroutineState::Suspended;
        co.yielded_value = Some(Value::int(42));
        assert!(matches!(co.state, CoroutineState::Suspended));
        assert_eq!(co.yielded_value, Some(Value::int(42)));

        // Transition to Done
        co.state = CoroutineState::Done;
        assert!(matches!(co.state, CoroutineState::Done));

        // Transition to Error
        co.state = CoroutineState::Error("test error".to_string());
        if let CoroutineState::Error(msg) = &co.state {
            assert_eq!(msg, "test error");
        } else {
            panic!("Expected Error state");
        }
    }
}
