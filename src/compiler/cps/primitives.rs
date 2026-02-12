//! CPS primitives: yield and resume

use crate::value::{Coroutine, CoroutineState, Value};
use std::rc::Rc;

/// Create a new coroutine from a closure
pub fn make_coroutine(closure: Value) -> Result<Value, String> {
    match closure {
        Value::Closure(c) => {
            let coroutine = Coroutine {
                closure: c,
                state: CoroutineState::Created,
                yielded_value: None,
            };
            Ok(Value::Coroutine(Rc::new(coroutine)))
        }
        Value::JitClosure(jc) => {
            // Convert JitClosure to Closure for coroutine
            if let Some(source) = &jc.source {
                let coroutine = Coroutine {
                    closure: source.clone(),
                    state: CoroutineState::Created,
                    yielded_value: None,
                };
                Ok(Value::Coroutine(Rc::new(coroutine)))
            } else {
                Err("JitClosure has no source for coroutine".to_string())
            }
        }
        _ => Err(format!("Cannot create coroutine from {:?}", closure)),
    }
}

/// Get the status of a coroutine
pub fn coroutine_status(coroutine: &Value) -> Result<Value, String> {
    match coroutine {
        Value::Coroutine(c) => {
            let status = match &c.state {
                CoroutineState::Created => "created",
                CoroutineState::Running => "running",
                CoroutineState::Suspended => "suspended",
                CoroutineState::Done => "done",
                CoroutineState::Error(_) => "error",
            };
            Ok(Value::String(status.to_string().into()))
        }
        _ => Err("Not a coroutine".to_string()),
    }
}

/// Check if a coroutine is done
pub fn coroutine_done(coroutine: &Value) -> Result<bool, String> {
    match coroutine {
        Value::Coroutine(c) => Ok(matches!(c.state, CoroutineState::Done)),
        _ => Err("Not a coroutine".to_string()),
    }
}

/// Get the last yielded value from a coroutine
pub fn coroutine_value(coroutine: &Value) -> Result<Value, String> {
    match coroutine {
        Value::Coroutine(c) => Ok(c.yielded_value.clone().unwrap_or(Value::Nil)),
        _ => Err("Not a coroutine".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::effects::Effect;
    use crate::value::{Arity, Closure};

    fn make_test_closure() -> Value {
        Value::Closure(Rc::new(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            source_ast: None,
            effect: Effect::Pure,
        }))
    }

    #[test]
    fn test_make_coroutine() {
        let closure = make_test_closure();
        let result = make_coroutine(closure);
        assert!(result.is_ok());

        if let Value::Coroutine(c) = result.unwrap() {
            assert!(matches!(c.state, CoroutineState::Created));
        } else {
            panic!("Expected coroutine");
        }
    }

    #[test]
    fn test_coroutine_status() {
        let closure = make_test_closure();
        let coroutine = make_coroutine(closure).unwrap();
        let status = coroutine_status(&coroutine).unwrap();
        assert_eq!(status, Value::String("created".to_string().into()));
    }

    #[test]
    fn test_coroutine_done() {
        let closure = make_test_closure();
        let coroutine = make_coroutine(closure).unwrap();
        assert!(!coroutine_done(&coroutine).unwrap());
    }
}
