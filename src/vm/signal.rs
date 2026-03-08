//! Primitive signal dispatch.
//!
//! Routes signal bits returned by NativeFn primitives to the appropriate
//! handler: stack push for SIG_OK, error storage for SIG_ERROR, fiber
//! execution for SIG_RESUME/SIG_PROPAGATE/SIG_CANCEL, VM state reads
//! for SIG_QUERY.

use crate::value::error_val;
use crate::value::{
    SignalBits, SuspendedFrame, Value, SIG_CANCEL, SIG_ERROR, SIG_OK, SIG_PROPAGATE, SIG_QUERY,
    SIG_RESUME, SIG_YIELD,
};
use std::rc::Rc;

use super::core::VM;

impl VM {
    /// Handle signal bits returned by a primitive in a Call position.
    ///
    /// Returns `None` to continue the dispatch loop, or `Some(bits)` to
    /// return from the dispatch loop (for yields/signals).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_primitive_signal(
        &mut self,
        bits: SignalBits,
        value: Value,
        bytecode: &Rc<Vec<u8>>,
        constants: &Rc<Vec<Value>>,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
        location_map: &Rc<crate::error::LocationMap>,
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
                // fiber/propagate: propagate the child fiber's signal
                self.handle_fiber_propagate_signal(value)
            }
            SIG_CANCEL => {
                // fiber/cancel: inject error into suspended fiber
                self.handle_fiber_cancel_signal(value, bytecode, constants, closure_env, ip)
            }
            SIG_QUERY => {
                // arena/allocs needs mutable VM access to call the thunk —
                // handle before dispatch_query (which takes &self).
                if let Some(cons) = value.as_cons() {
                    if cons.first.as_keyword_name() == Some("arena/allocs") {
                        let thunk = cons.rest;
                        match self.handle_arena_allocs(thunk) {
                            Ok(val) => {
                                self.fiber.stack.push(val);
                                return None;
                            }
                            Err(bits) => return Some(bits),
                        }
                    }
                }
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
                // Any yielding signal (SIG_YIELD, SIG_YIELD|SIG_IO, user-defined suspension).
                // Save the stack into a SuspendedFrame so call.rs can build the caller
                // frame chain on the way out. Non-yielding signals don't need a frame.
                if bits.contains(SIG_YIELD) {
                    let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                    let frame = SuspendedFrame {
                        bytecode: bytecode.clone(),
                        constants: constants.clone(),
                        env: closure_env.clone(),
                        ip: *ip,
                        stack: saved_stack,
                        active_allocator: crate::value::fiber_heap::save_active_allocator(),
                        location_map: location_map.clone(),
                    };
                    self.fiber.signal = Some((bits, value));
                    self.fiber.suspended = Some(vec![frame]);
                } else {
                    self.fiber.signal = Some((bits, value));
                }
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
                // arena/allocs needs mutable VM access to call the thunk —
                // handle before dispatch_query (which takes &self).
                if let Some(cons) = value.as_cons() {
                    if cons.first.as_keyword_name() == Some("arena/allocs") {
                        let thunk = cons.rest;
                        match self.handle_arena_allocs(thunk) {
                            Ok(val) => {
                                self.fiber.signal = Some((SIG_OK, val));
                                return SIG_OK;
                            }
                            Err(bits) => return bits,
                        }
                    }
                }
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
                    .and_then(|closure| closure.doc)
                    .and_then(|doc_val| doc_val.with_string(|s| s.to_string()));
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
                use crate::value::heap::{
                    heap_arena_capacity, heap_arena_len, heap_arena_object_limit, heap_arena_peak,
                    TableKey,
                };
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
                let limit_val = match heap_arena_object_limit() {
                    Some(n) => Value::int(n as i64),
                    None => Value::NIL,
                };
                fields.insert(TableKey::Keyword("object-limit".to_string()), limit_val);
                // Bytes: estimate from object count × 128 bytes per object
                fields.insert(
                    TableKey::Keyword("bytes".to_string()),
                    Value::int((heap_arena_len() * 128) as i64),
                );
                // Peak object count (high-water mark)
                let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
                let peak = if heap_ptr.is_null() {
                    heap_arena_peak() as i64
                } else {
                    unsafe { (*heap_ptr).peak_alloc_count() as i64 }
                };
                fields.insert(TableKey::Keyword("peak".to_string()), Value::int(peak));
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
                (SIG_OK, Value::struct_from(fields))
            }
            "arena/fiber-stats" => {
                let fiber_handle = match arg.as_fiber() {
                    Some(h) => h,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                "arena/fiber-stats: expected a fiber".to_string(),
                            ),
                        )
                    }
                };
                match fiber_handle.try_with(|fiber| {
                    use crate::value::heap::TableKey;
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("count".to_string()),
                        Value::int(fiber.heap.len() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("bytes".to_string()),
                        Value::int(fiber.heap.allocated_bytes() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("peak".to_string()),
                        Value::int(fiber.heap.peak_alloc_count() as i64),
                    );
                    let limit_val = match fiber.heap.object_limit() {
                        Some(n) => Value::int(n as i64),
                        None => Value::NIL,
                    };
                    fields.insert(TableKey::Keyword("object-limit".to_string()), limit_val);
                    fields.insert(
                        TableKey::Keyword("scope-enters".to_string()),
                        Value::int(fiber.heap.scope_enters() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("dtors-run".to_string()),
                        Value::int(fiber.heap.scope_dtors_run() as i64),
                    );
                    Value::struct_from(fields)
                }) {
                    Some(val) => (SIG_OK, val),
                    None => (
                        SIG_ERROR,
                        error_val(
                            "state-error",
                            "arena/fiber-stats: cannot inspect a currently-executing fiber"
                                .to_string(),
                        ),
                    ),
                }
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
}

impl VM {
    /// Handle `arena/allocs` — snapshot count, call thunk, snapshot again.
    ///
    /// Uses `execute_bytecode_saving_stack` (re-entrant VM call). The thunk
    /// runs on the current fiber — same heap, same globals, same parameter
    /// frames. Yield from the thunk is propagated upward (not handled here);
    /// callers should only pass non-yielding (Pure effect) closures.
    ///
    /// The before/after count snapshots bracket the thunk's execution to
    /// measure net allocations.
    ///
    /// Returns `Ok(cons(result, net_allocs))` on success, or `Err(bits)` on error/halt.
    pub(crate) fn handle_arena_allocs(&mut self, thunk: Value) -> Result<Value, SignalBits> {
        let closure = match thunk.as_closure() {
            Some(c) => c.clone(),
            None => {
                let err = error_val("type-error", "arena/allocs: expected a closure");
                self.fiber.signal = Some((SIG_ERROR, err));
                self.fiber.stack.push(Value::NIL);
                return Err(SIG_ERROR);
            }
        };

        // Snapshot count before
        let before = {
            let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
            if heap_ptr.is_null() {
                crate::value::heap::heap_arena_len()
            } else {
                unsafe { (*heap_ptr).len() }
            }
        };

        // Build a proper env (captures + local slots) for the thunk.
        // Passing closure.env directly would omit local variable slots,
        // causing StoreUpvalue panics for closures with locals.
        let thunk_env = self
            .build_closure_env(&closure, &[])
            .expect("arena/allocs: zero-arg thunk env build cannot fail");

        // Execute the thunk via execute_bytecode_saving_stack
        let exec_result = self.execute_bytecode_saving_stack(
            &closure.bytecode,
            &closure.constants,
            &thunk_env,
            &closure.location_map,
        );

        if exec_result.bits.contains(SIG_ERROR) {
            // Propagate the error — signal is already set by the inner execution
            return Err(exec_result.bits);
        }

        // Get result from signal
        let result = self
            .fiber
            .signal
            .take()
            .map(|(_, v)| v)
            .unwrap_or(Value::NIL);

        // Snapshot count after
        let after = {
            let heap_ptr = crate::value::fiber_heap::current_heap_ptr();
            if heap_ptr.is_null() {
                crate::value::heap::heap_arena_len()
            } else {
                unsafe { (*heap_ptr).len() }
            }
        };

        let net = (after as i64) - (before as i64);
        Ok(Value::cons(result, Value::int(net)))
    }
}
