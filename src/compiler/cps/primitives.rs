//! CPS primitives: yield and resume

use crate::value::heap::{alloc, HeapObject};
use crate::value::{Coroutine, CoroutineState, Value};
use std::rc::Rc;

use std::sync::{Arc, Mutex};
// Helper function to convert from old Value to new Value
pub fn old_value_to_new(old_val: &crate::value_old::Value) -> Value {
    use crate::value_old::Value as OldValue;
    match old_val {
        OldValue::Nil => Value::NIL,
        OldValue::Bool(b) => Value::bool(*b),
        OldValue::Int(i) => Value::int(*i),
        OldValue::Float(f) => Value::float(*f),
        OldValue::Symbol(id) => Value::symbol(id.0),
        OldValue::Keyword(id) => Value::keyword(id.0),
        OldValue::String(s) => Value::string(s.as_ref()),
        OldValue::Cons(cons) => {
            let first = old_value_to_new(&cons.first);
            let rest = old_value_to_new(&cons.rest);
            crate::value::cons(first, rest)
        }
        OldValue::Table(t) => {
            let borrowed = t.borrow();
            let mut new_table = std::collections::BTreeMap::new();
            for (k, v) in borrowed.iter() {
                new_table.insert(k.clone(), old_value_to_new(v));
            }
            Value::table_from(new_table)
        }
        OldValue::Struct(s) => {
            let mut new_struct = std::collections::BTreeMap::new();
            for (k, v) in s.iter() {
                new_struct.insert(k.clone(), old_value_to_new(v));
            }
            Value::struct_from(new_struct)
        }
        OldValue::Vector(v) => {
            let new_vals: Vec<Value> = v.iter().map(old_value_to_new).collect();
            Value::vector(new_vals)
        }
        OldValue::Closure(c) => alloc(HeapObject::Closure(c.clone())),
        OldValue::JitClosure(jc) => alloc(HeapObject::JitClosure(jc.clone())),
        OldValue::NativeFn(f) => alloc(HeapObject::NativeFn(*f)),
        OldValue::VmAwareFn(f) => alloc(HeapObject::VmAwareFn(*f)),
        OldValue::LibHandle(h) => alloc(HeapObject::LibHandle(h.0)),
        OldValue::CHandle(h) => alloc(HeapObject::CHandle(h.ptr, h.id)),
        OldValue::Condition(c) => alloc(HeapObject::Condition(c.as_ref().clone())),
        OldValue::ThreadHandle(_h) => alloc(HeapObject::ThreadHandle(
            crate::value::heap::ThreadHandleData {
                result: Arc::new(Mutex::new(None)),
            },
        )),
        OldValue::Cell(c) => {
            let borrowed = c.borrow();
            let inner = old_value_to_new(&borrowed);
            Value::cell(inner)
        }
        OldValue::LocalCell(c) => {
            let borrowed = c.borrow();
            let inner = old_value_to_new(&borrowed);
            Value::cell(inner)
        }
        OldValue::Coroutine(co) => {
            // co is Rc<RefCell<Coroutine>>, we need RefCell<Coroutine>
            let borrowed = co.borrow();
            alloc(HeapObject::Coroutine(Rc::new(std::cell::RefCell::new(
                borrowed.clone(),
            ))))
        }
    }
}

/// Create a new coroutine from a closure
pub fn make_coroutine(closure: Value) -> Result<Value, String> {
    if let Some(c) = closure.as_closure() {
        let coroutine = Coroutine::new((*c).clone());
        Ok(alloc(HeapObject::Coroutine(Rc::new(
            std::cell::RefCell::new(coroutine),
        ))))
    } else if let Some(jc) = closure.as_jit_closure() {
        // Convert JitClosure to Closure for coroutine
        if let Some(source) = &jc.source {
            let coroutine = Coroutine::new(source.clone());
            Ok(alloc(HeapObject::Coroutine(Rc::new(
                std::cell::RefCell::new(coroutine),
            ))))
        } else {
            Err("JitClosure has no source for coroutine".to_string())
        }
    } else {
        Err(format!("Cannot create coroutine from {:?}", closure))
    }
}

/// Get the status of a coroutine
pub fn coroutine_status(coroutine: &Value) -> Result<Value, String> {
    use crate::value::heap::{deref, HeapObject};

    if !coroutine.is_heap() {
        return Err("Not a coroutine".to_string());
    }

    match unsafe { deref(*coroutine) } {
        HeapObject::Coroutine(c) => {
            let borrowed = c.borrow();
            let status = match &borrowed.state {
                CoroutineState::Created => "created",
                CoroutineState::Running => "running",
                CoroutineState::Suspended => "suspended",
                CoroutineState::Done => "done",
                CoroutineState::Error(_) => "error",
            };
            Ok(Value::string(status))
        }
        _ => Err("Not a coroutine".to_string()),
    }
}

/// Check if a coroutine is done
pub fn coroutine_done(coroutine: &Value) -> Result<bool, String> {
    use crate::value::heap::{deref, HeapObject};

    if !coroutine.is_heap() {
        return Err("Not a coroutine".to_string());
    }

    match unsafe { deref(*coroutine) } {
        HeapObject::Coroutine(c) => {
            let borrowed = c.borrow();
            Ok(matches!(borrowed.state, CoroutineState::Done))
        }
        _ => Err("Not a coroutine".to_string()),
    }
}

/// Get the last yielded value from a coroutine
pub fn coroutine_value(coroutine: &Value) -> Result<Value, String> {
    use crate::value::heap::{deref, HeapObject};

    if !coroutine.is_heap() {
        return Err("Not a coroutine".to_string());
    }

    match unsafe { deref(*coroutine) } {
        HeapObject::Coroutine(c) => {
            let borrowed = c.borrow();
            Ok(borrowed
                .yielded_value
                .as_ref()
                .map(old_value_to_new)
                .unwrap_or(Value::NIL))
        }
        _ => Err("Not a coroutine".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::value::heap::{deref, HeapObject};
    use crate::value::{Arity, Closure};
    use std::rc::Rc;

    fn make_test_closure() -> Value {
        Value::closure(Closure {
            bytecode: Rc::new(vec![]),
            arity: Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            source_ast: None,
            effect: Effect::Pure,
            cell_params_mask: 0,
            symbol_names: Rc::new(std::collections::HashMap::new()),
        })
    }

    #[test]
    fn test_make_coroutine() {
        let closure = make_test_closure();
        let result = make_coroutine(closure);
        assert!(result.is_ok());

        let coroutine = result.unwrap();
        if let HeapObject::Coroutine(c) = unsafe { deref(coroutine) } {
            let borrowed = c.borrow();
            assert!(matches!(borrowed.state, CoroutineState::Created));
        } else {
            panic!("Expected coroutine");
        }
    }

    #[test]
    fn test_coroutine_status() {
        let closure = make_test_closure();
        let coroutine = make_coroutine(closure).unwrap();
        let status = coroutine_status(&coroutine).unwrap();
        assert_eq!(status, Value::string("created"));
    }

    #[test]
    fn test_coroutine_done() {
        let closure = make_test_closure();
        let coroutine = make_coroutine(closure).unwrap();
        assert!(!coroutine_done(&coroutine).unwrap());
    }
}
