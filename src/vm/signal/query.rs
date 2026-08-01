use super::*;

impl VM {
    /// Dispatch a VM state query. Value is (operation . argument).
    ///
    /// The operation can be a keyword or a string. Keywords are resolved
    /// via the content-addressed keyword registry; strings are used
    /// directly. SIG_QUERY is for questions that can only be answered
    /// from the VM's context (call counts, documentation, current fiber).
    ///
    /// Operations:
    /// - (:"call-count" . closure) — return call count for closure
    /// - (:"doc" . name) — return formatted documentation for a primitive
    /// - (:"global?" . symbol) — always false (no runtime globals exist)
    /// - (:"fiber/self" . _) — return the currently executing fiber, or nil
    /// - (:"list-primitives" . _) — return sorted list of all primitive names
    /// - (:"primitive-meta" . name) — return struct with primitive metadata
    /// - (:"arena/stats" . nil) — return unified stats struct (12 fields) for current fiber
    /// - (:"arena/stats" . fiber) — return unified stats struct for a suspended/dead fiber
    /// - (:"arena/count" . _) — return heap arena object count as int (zero overhead)
    /// - (:"jit?" . closure) — true if closure has JIT-compiled native code
    pub(crate) fn dispatch_query(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        value: Value,
    ) -> (SignalBits, Value) {
        let pair = match value.as_pair() {
            Some(c) => c,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "SIG_QUERY: expected pair cell".to_string()),
                );
            }
        };

        // Accept keyword or string as operation identifier.
        let op_name: String = if let Some(name) = pair.first.as_keyword_name() {
            name
        } else if let Some(s) = pair.first.with_string(|s| s.to_string()) {
            s
        } else {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "SIG_QUERY: operation must be a keyword or string".to_string(),
                ),
            );
        };
        let arg = pair.rest;

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
                // No global mutable state — always false.
                let _ = arg;
                (SIG_OK, Value::FALSE)
            }
            "doc" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s
                } else {
                    return (
                        SIG_ERROR,
                        ctx.error("type-error", "doc: expected string or keyword".to_string()),
                    );
                };
                // Look up builtin docs by name. Stdlib closures are handled
                // upstream: the analyzer passes them through as closure values,
                // and prim_doc extracts the docstring from closure.template.doc
                // before the SIG_QUERY reaches here. This path is only reached
                // for native primitives, special forms, and explicit string args.
                if let Some(doc) = self.docs.get(&name) {
                    (SIG_OK, ctx.string(doc.format()))
                } else {
                    (
                        SIG_OK,
                        ctx.string(format!("No documentation found for '{}'", name)),
                    )
                }
            }
            "fiber/self" => (SIG_OK, self.current_fiber_value.unwrap_or(Value::NIL)),
            "fiber/caps" => {
                let caps = crate::signals::CAP_MASK.subtract(self.fiber.withheld);
                let registry = crate::signals::registry::global_registry().lock().unwrap();
                let keywords = registry.bits_to_keywords(caps);
                (SIG_OK, ctx.set(keywords.into_iter().collect()))
            }
            "list-primitives" => {
                // arg is nil (no filter) or a keyword/string category name
                let category_filter: Option<String> = if arg.is_nil() {
                    None
                } else if let Some(k) = arg.as_keyword_name() {
                    Some(k)
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
                // This instance's own table. Copied out as a raw pointer so the
                // per-name intern stays independent of the `&self` borrow `names`
                // already holds on `self.docs`.
                let symbols_ptr = self.symbols_ptr;
                let values: Vec<Value> = names
                    .iter()
                    .map(|n| {
                        if symbols_ptr.is_null() {
                            ctx.string(n)
                        } else {
                            let id = unsafe { (*symbols_ptr).intern(n) };
                            Value::symbol(id.0)
                        }
                    })
                    .collect();
                (SIG_OK, ctx.list(values))
            }
            "primitive-meta" => {
                let name = if let Some(s) = arg.with_string(|s| s.to_string()) {
                    s
                } else if let Some(s) = arg.as_keyword_name() {
                    s
                } else if let Some(sym_id) = arg.as_symbol() {
                    match self.symbols().and_then(|s| {
                        s.name(crate::value::SymbolId(sym_id))
                            .map(|n| n.to_string())
                    }) {
                        Some(s) => s,
                        None => {
                            return (
                                SIG_ERROR,
                                ctx.error(
                                    "internal-error",
                                    format!(
                                        "primitive-meta: symbol ID {} not found in symbol table",
                                        sym_id
                                    ),
                                ),
                            );
                        }
                    }
                } else {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "type-error",
                            "primitive-meta: expected string, keyword, or symbol".to_string(),
                        ),
                    );
                };
                if let Some(doc) = self.docs.get(&name) {
                    use crate::value::heap::TableKey;
                    use std::collections::BTreeMap;
                    let mut fields = BTreeMap::new();
                    fields.insert(TableKey::Keyword("name".to_string()), ctx.string(doc.name));
                    fields.insert(TableKey::Keyword("doc".to_string()), ctx.string(doc.doc));
                    // params as a list of strings
                    let params: Vec<Value> = doc.params.iter().map(|p| ctx.string(*p)).collect();
                    fields.insert(TableKey::Keyword("params".to_string()), ctx.list(params));
                    fields.insert(
                        TableKey::Keyword("category".to_string()),
                        ctx.string(doc.category),
                    );
                    fields.insert(
                        TableKey::Keyword("example".to_string()),
                        ctx.string(doc.example),
                    );
                    fields.insert(
                        TableKey::Keyword("arity".to_string()),
                        ctx.string(format!("{}", doc.arity)),
                    );
                    fields.insert(
                        TableKey::Keyword("signal".to_string()),
                        ctx.string(format!("{}", doc.signal)),
                    );
                    // aliases as a list of strings
                    let aliases: Vec<Value> = doc.aliases.iter().map(|a| ctx.string(*a)).collect();
                    fields.insert(TableKey::Keyword("aliases".to_string()), ctx.list(aliases));
                    (SIG_OK, ctx.struct_from(fields))
                } else {
                    (SIG_OK, Value::NIL)
                }
            }
            "arena/stats" => {
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;

                /// Read the unified stats fields from a FiberHeap reference. The
                /// caller births the struct through `ctx` so the stats read (a
                /// shared `&FiberHeap`) and the allocation (`ctx`'s own heap) never
                /// overlap. Fields: :object-count, :peak-count, :allocated-bytes,
                /// :object-limit, :scope-depth, :active-allocator,
                /// :scope-enter-count, :scope-dtor-count.
                fn build_stats(heap: &crate::value::FiberHeap) -> BTreeMap<TableKey, Value> {
                    let mut fields = BTreeMap::new();
                    fields.insert(
                        TableKey::Keyword("object-count".to_string()),
                        Value::int(heap.visible_len() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("peak-count".to_string()),
                        Value::int(heap.peak_alloc_count() as i64),
                    );
                    fields.insert(
                        TableKey::Keyword("allocated-bytes".to_string()),
                        Value::int(heap.allocated_bytes() as i64),
                    );
                    let limit_val = match heap.object_limit() {
                        Some(n) => Value::int(n as i64),
                        None => Value::NIL,
                    };
                    fields.insert(TableKey::Keyword("object-limit".to_string()), limit_val);
                    fields.insert(TableKey::Keyword("scope-depth".to_string()), Value::int(0));
                    fields.insert(
                        TableKey::Keyword("active-allocator".to_string()),
                        Value::keyword("region"),
                    );
                    fields.insert(
                        TableKey::Keyword("scope-enter-count".to_string()),
                        Value::int(0),
                    );
                    fields.insert(
                        TableKey::Keyword("scope-dtor-count".to_string()),
                        Value::int(0),
                    );
                    fields
                }

                if arg.is_nil() {
                    // 0-arg path: read from this instance's own heap.
                    let stats = ctx.struct_from(unsafe { build_stats(&*self.heap_ptr) });
                    (SIG_OK, stats)
                } else {
                    // 1-arg path: validate it's a fiber, then return this
                    // instance's heap stats (all of an instance's fibers share
                    // its one heap).
                    if arg.as_fiber().is_none() {
                        return type_error!(ctx, arg, "arena/stats", "fiber");
                    }
                    let stats = ctx.struct_from(unsafe { build_stats(&*self.heap_ptr) });
                    (SIG_OK, stats)
                }
            }
            #[cfg(feature = "jit")]
            "jit/rejections" => {
                use crate::value::heap::TableKey;
                use std::collections::BTreeMap;

                // Drain pending background compilations so all rejections
                // are available before reporting.
                self.drain_jit_pending();

                // Sort by call count ascending (coldest first, hottest last).
                let mut entries: Vec<_> = self.jit_rejections.iter().collect();
                entries.sort_by_key(|(ptr, _)| {
                    self.closure_call_counts.get(ptr).copied().unwrap_or(0)
                });

                let structs: Vec<Value> = entries
                    .into_iter()
                    .map(|(ptr, info)| {
                        let mut fields = BTreeMap::new();
                        let name = info.name.as_deref().unwrap_or("<anon>");
                        fields.insert(TableKey::Keyword("name".to_string()), ctx.string(name));
                        fields.insert(
                            TableKey::Keyword("reason".to_string()),
                            ctx.string(info.reason.to_string()),
                        );
                        let calls = self.closure_call_counts.get(ptr).copied().unwrap_or(0);
                        fields.insert(
                            TableKey::Keyword("calls".to_string()),
                            Value::int(calls as i64),
                        );
                        let attempts = self.jit_compile_attempts.get(ptr).copied().unwrap_or(0);
                        fields.insert(
                            TableKey::Keyword("attempts".to_string()),
                            Value::int(attempts as i64),
                        );
                        ctx.struct_from(fields)
                    })
                    .collect();
                (SIG_OK, ctx.list(structs))
            }
            #[cfg(not(feature = "jit"))]
            "jit/rejections" => (SIG_OK, ctx.list(vec![])),
            #[cfg(feature = "jit")]
            "jit?" => {
                if let Some(closure) = arg.as_closure() {
                    let ptr = closure.template.bytecode.as_ptr();
                    (SIG_OK, Value::bool(self.jit_cache.contains_key(&ptr)))
                } else {
                    (SIG_OK, Value::FALSE)
                }
            }
            #[cfg(not(feature = "jit"))]
            "jit?" => (SIG_OK, Value::FALSE),
            "vm/config" => self.dispatch_vm_config_read(ctx, arg),
            #[cfg(feature = "mlir")]
            "mlir/compile-spirv" => {
                // arg is (closure . workgroup-size)
                let (closure_val, wg_size): (Value, u32) = match arg.as_pair() {
                    Some(c) => (c.first, c.rest.as_int().unwrap_or(256) as u32),
                    None => (arg, 256),
                };

                let closure = match closure_val.as_closure() {
                    Some(c) => c,
                    None => return type_error!(ctx, closure_val, "mlir/compile-spirv", "closure"),
                };
                let lir = match &closure.template.lir_function {
                    Some(lir) => lir,
                    None => {
                        return (
                            SIG_ERROR,
                            ctx.error(
                                "mlir-error",
                                "mlir/compile-spirv: closure has no LIR".to_string(),
                            ),
                        )
                    }
                };
                if !lir.is_gpu_eligible() {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "mlir-error",
                            "mlir/compile-spirv: closure is not GPU-eligible".to_string(),
                        ),
                    );
                }
                let key = closure.template.bytecode.as_ptr();
                let cache = self
                    .mlir_cache
                    .get_or_insert_with(crate::mlir::MlirCache::new);
                match cache.compile_spirv(key, lir, wg_size) {
                    Ok(bytes) => (SIG_OK, ctx.bytes(bytes.to_vec())),
                    Err(e) => (
                        SIG_ERROR,
                        ctx.error("mlir-error", format!("mlir/compile-spirv: {}", e)),
                    ),
                }
            }
            #[cfg(feature = "mlir")]
            "git" => {
                // arg is (closure . workgroup-size)
                let (closure_val, wg_size): (Value, u32) = match arg.as_pair() {
                    Some(c) => (c.first, c.rest.as_int().unwrap_or(256) as u32),
                    None => (arg, 256),
                };

                let closure = match closure_val.as_closure() {
                    Some(c) => c,
                    None => return type_error!(ctx, closure_val, "git", "closure"),
                };
                // Already cached? Return early.
                if closure.template.spirv.get().is_some() {
                    return (SIG_OK, closure_val);
                }
                let lir = match &closure.template.lir_function {
                    Some(lir) => lir,
                    None => {
                        return (
                            SIG_ERROR,
                            ctx.error("mlir-error", "git: closure has no LIR".to_string()),
                        )
                    }
                };
                if !lir.is_gpu_eligible() {
                    return (
                        SIG_ERROR,
                        ctx.error("mlir-error", "git: closure is not GPU-eligible".to_string()),
                    );
                }
                let key = closure.template.bytecode.as_ptr();
                let cache = self
                    .mlir_cache
                    .get_or_insert_with(crate::mlir::MlirCache::new);
                match cache.compile_spirv(key, lir, wg_size) {
                    Ok(bytes) => {
                        // Cache on the template (OnceCell — idempotent).
                        let _ = closure.template.spirv.set(bytes.to_vec());
                        (SIG_OK, closure_val)
                    }
                    Err(e) => (SIG_ERROR, ctx.error("mlir-error", format!("git: {}", e))),
                }
            }
            "compile/run-on" => self.dispatch_compile_run_on(ctx, arg),
            "compile/barrier-module" => self.dispatch_barrier_module(ctx, arg),
            "compile/whole-module" => self.dispatch_whole_module(ctx, arg),
            "compile/whole-module-syntax" => self.dispatch_whole_module_syntax(ctx, arg),
            "compile/dumps" => self.dispatch_compile_dumps(ctx, arg),
            "arena/allocs" => self.handle_arena_allocs(ctx, arg),
            "vm/config-set" => (SIG_OK, self.handle_vm_config_set(ctx, arg)),
            _ => (
                SIG_ERROR,
                ctx.error(
                    "argument-error",
                    format!("SIG_QUERY: unknown operation: {}", op_name),
                ),
            ),
        }
    }
}
