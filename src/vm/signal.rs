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
    /// - (:"doc" . name) — return formatted documentation for a primitive
    /// - (:"global?" . symbol) — return true if symbol is bound as a global
    /// - (:"fiber/self" . _) — return the currently executing fiber, or nil
    /// - (:"list-primitives" . _) — return sorted list of all primitive names
    /// - (:"primitive-meta" . name) — return struct with primitive metadata
    /// - (:"arena/stats" . _) — return struct with heap arena :count and :capacity
    /// - (:"arena/count" . _) — return heap arena object count as int (zero overhead)
    /// - (:"arena/scope-stats" . _) — return scope allocation stats {:enters N :dtors-run N}
    /// - (:"environment" . _) — return global environment as struct
    pub(crate) fn dispatch_query(&self, value: Value) -> (SignalBits, Value) {
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
        } else if let Some(s) = cons.first.with_string(|s| s.to_string()) {
            s
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
                    let ptr = closure.template.bytecode.as_ptr();
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
            "doc" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s.to_string()
                } else {
                    return (
                        SIG_ERROR,
                        error_val("type-error", "doc: expected string or keyword".to_string()),
                    );
                };
                // First, check if the named global is a closure with a doc field
                let user_doc = unsafe { crate::context::get_symbol_table() }
                    .and_then(|st_ptr| unsafe { (*st_ptr).get(&name) })
                    .and_then(|sym_id| self.get_global(sym_id.0))
                    .and_then(|val| val.as_closure().cloned())
                    .and_then(|closure| closure.template.doc)
                    .and_then(|doc_val| doc_val.with_string(|s| s.to_string()));
                // If not found in globals, check file-level locals (letrec model)
                let user_doc = user_doc.or_else(|| {
                    self.get_local_value_by_name(&name)
                        .and_then(|val| val.as_closure().cloned())
                        .and_then(|closure| closure.template.doc)
                        .and_then(|doc_val| doc_val.with_string(|s| s.to_string()))
                });
                if let Some(doc_str) = user_doc {
                    (SIG_OK, Value::string(doc_str))
                } else if let Some(doc) = self.docs.get(&name) {
                    (SIG_OK, Value::string(doc.format()))
                } else {
                    (
                        SIG_OK,
                        Value::string(format!("No documentation found for '{}'", name)),
                    )
                }
            }
            "fiber/self" => (SIG_OK, self.current_fiber_value.unwrap_or(Value::NIL)),
            "list-primitives" => {
                // arg is nil (no filter) or a keyword/string category name
                let category_filter: Option<String> = if arg.is_nil() {
                    None
                } else if let Some(k) = arg.as_keyword_name() {
                    Some(k.to_string())
                } else {
                    arg.with_string(|s| s.to_string())
                };

                let mut names: Vec<&String> = if let Some(ref cat) = category_filter {
                    self.docs
                        .iter()
                        .filter(|(_, doc)| doc.category == cat.as_str())
                        .map(|(name, _)| name)
                        .collect()
                } else {
                    self.docs.keys().collect()
                };
                names.sort();
                let values: Vec<Value> =
                    names.iter().map(|n| Value::string(n.to_string())).collect();
                (SIG_OK, crate::value::list(values))
            }
            "primitive-meta" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s.to_string()
                } else {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            "primitive-meta: expected string or keyword".to_string(),
                        ),
                    );
                };
                if let Some(doc) = self.docs.get(&name) {
                    use crate::value::heap::TableKey;
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("name".to_string()),
                        Value::string(doc.name.to_string()),
                    );
                    fields.insert(
                        TableKey::Keyword("doc".to_string()),
                        Value::string(doc.doc.to_string()),
                    );
                    // params as a list of strings
                    let params: Vec<Value> = doc
                        .params
                        .iter()
                        .map(|p| Value::string(p.to_string()))
                        .collect();
                    fields.insert(
                        TableKey::Keyword("params".to_string()),
                        crate::value::list(params),
                    );
                    fields.insert(
                        TableKey::Keyword("category".to_string()),
                        Value::string(doc.category.to_string()),
                    );
                    fields.insert(
                        TableKey::Keyword("example".to_string()),
                        Value::string(doc.example.to_string()),
                    );
                    fields.insert(
                        TableKey::Keyword("arity".to_string()),
                        Value::string(format!("{}", doc.arity)),
                    );
                    fields.insert(
                        TableKey::Keyword("effect".to_string()),
                        Value::string(format!("{}", doc.effect)),
                    );
                    // aliases as a list of strings
                    let aliases: Vec<Value> = doc
                        .aliases
                        .iter()
                        .map(|a| Value::string(a.to_string()))
                        .collect();
                    fields.insert(
                        TableKey::Keyword("aliases".to_string()),
                        crate::value::list(aliases),
                    );
                    (SIG_OK, Value::struct_from(fields))
                } else {
                    (SIG_OK, Value::NIL)
                }
            }
            "arena/stats" => {
                use crate::value::heap::{heap_arena_capacity, heap_arena_len, TableKey};
                use std::collections::BTreeMap;
                let mut fields = BTreeMap::new();
                fields.insert(
                    TableKey::Keyword("count".to_string()),
                    Value::int(heap_arena_len() as i64),
                );
                fields.insert(
                    TableKey::Keyword("capacity".to_string()),
                    Value::int(heap_arena_capacity() as i64),
                );
                (SIG_OK, Value::struct_from(fields))
            }
            "arena/count" => {
                use crate::value::heap::heap_arena_len;
                (SIG_OK, Value::int(heap_arena_len() as i64))
            }
            "arena/scope-stats" => {
                use crate::value::fiber_heap::with_current_heap_mut;
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;
                let (enters, dtors_run) =
                    with_current_heap_mut(|heap| (heap.scope_enters(), heap.scope_dtors_run()))
                        .unwrap_or((0, 0));
                let mut fields = BTreeMap::new();
                fields.insert(
                    TableKey::Keyword("enters".to_string()),
                    Value::int(enters as i64),
                );
                fields.insert(
                    TableKey::Keyword("dtors-run".to_string()),
                    Value::int(dtors_run as i64),
                );
                (SIG_OK, Value::struct_from(fields))
            }
            "environment" => {
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;
                let mut fields = BTreeMap::new();
                let st_ptr = unsafe { crate::context::get_symbol_table() };
                if let Some(st_ptr) = st_ptr {
                    let st = unsafe { &*st_ptr };
                    for (idx, &defined) in self.defined_globals.iter().enumerate() {
                        if !defined {
                            continue;
                        }
                        if let Some(name) = st.name(crate::value::SymbolId(idx as u32)) {
                            fields.insert(TableKey::Keyword(name.to_string()), self.globals[idx]);
                        }
                    }
                }
                // Also include file-level locals (letrec model)
                let frame_base = self.current_frame_base();
                for (&slot, name) in &self.local_names {
                    let abs_idx = frame_base + slot as usize;
                    if abs_idx < self.fiber.stack.len() {
                        let val = self.unwrap_local_cell(self.fiber.stack[abs_idx]);
                        fields.insert(TableKey::Keyword(name.clone()), val);
                    }
                }
                (SIG_OK, Value::struct_from(fields))
            }
            _ => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("SIG_QUERY: unknown operation: {}", op_name),
                ),
            ),
        }
    }

    /// Look up a file-level local variable by name.
    ///
    /// Uses `self.local_names` (slot → name mapping from `compile_file`)
    /// to find the slot, then reads the value from the stack. Unwraps
    /// `LocalCell` for mutable bindings.
    fn get_local_value_by_name(&self, name: &str) -> Option<Value> {
        let frame_base = self.current_frame_base();
        for (&slot, slot_name) in &self.local_names {
            if slot_name == name {
                let abs_idx = frame_base + slot as usize;
                if abs_idx < self.fiber.stack.len() {
                    return Some(self.unwrap_local_cell(self.fiber.stack[abs_idx]));
                }
            }
        }
        None
    }

    /// Unwrap a `LocalCell` to get the inner value, or return as-is.
    fn unwrap_local_cell(&self, val: Value) -> Value {
        if val.is_local_cell() {
            if let Some(cell) = val.as_cell() {
                return *cell.borrow();
            }
        }
        val
    }
}
