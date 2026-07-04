use super::*;

/// (compile/extract analysis {:from :fn :lines [s e] :name :new}) → {:source :new-function :captures :signal}
pub(crate) fn prim_compile_extract(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/extract", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let opts = match args[1].as_struct() {
        Some(f) => f,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "compile/extract: second argument must be a struct",
                ),
            )
        }
    };

    let from_name = match sorted_struct_get(opts, &kw("from")).and_then(|v| {
        v.as_keyword_name()
            .map(|s| s.to_string())
            .or_else(|| v.with_string(|s| s.to_string()))
    }) {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "compile/extract: :from is required"),
            )
        }
    };

    let (start_line, end_line) =
        match sorted_struct_get(opts, &kw("lines")).and_then(|v| v.as_array()) {
            Some(arr) if arr.len() == 2 => {
                let s = arr[0].as_int().unwrap_or(0) as u32;
                let e = arr[1].as_int().unwrap_or(0) as u32;
                (s, e)
            }
            _ => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "compile/extract: :lines must be [start end]"),
                )
            }
        };

    let new_fn_name = match sorted_struct_get(opts, &kw("name")).and_then(|v| {
        v.as_keyword_name()
            .map(|s| s.to_string())
            .or_else(|| v.with_string(|s| s.to_string()))
    }) {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "compile/extract: :name is required"),
            )
        }
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/extract: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    if start_line == 0 || end_line == 0 || start_line > end_line {
        return (
            SIG_ERROR,
            ctx.error("range-error", "compile/extract: invalid line range"),
        );
    }

    let line_offsets = compute_line_offsets(&handle.source);
    let start_byte = line_offsets
        .get((start_line - 1) as usize)
        .copied()
        .unwrap_or(0);
    let end_byte = line_offsets
        .get(end_line as usize)
        .copied()
        .unwrap_or(handle.source.len());

    // Find the lambda and collect free vars in range.
    let lambda = match find_named_lambda(&handle.hir, &handle.arena, symbols, &from_name) {
        Some(l) => l,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "lookup-error",
                    format!("compile/extract: no function '{}'", from_name),
                ),
            )
        }
    };

    let mut referenced = BTreeSet::new();
    let mut defined = BTreeSet::new();
    let mut signal = Signal::silent();

    if let HirKind::Lambda { body, .. } = &lambda.kind {
        collect_vars_in_range(
            body,
            start_byte,
            end_byte,
            &mut referenced,
            &mut defined,
            &mut signal,
        );
    }

    let free_vars: Vec<String> = referenced
        .difference(&defined)
        .filter(|b| !handle.arena.get(**b).is_primitive)
        .filter_map(|b| {
            symbols
                .name(handle.arena.get(*b).name)
                .map(|s| s.to_string())
        })
        .collect();

    let extracted_body = handle.source[start_byte..end_byte].trim();

    let params_str = if free_vars.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", free_vars.join(" "))
    };
    let new_function = format!(
        "(defn {} {}\n  {})",
        new_fn_name, params_str, extracted_body
    );

    let replacement = if free_vars.is_empty() {
        format!("({})", new_fn_name)
    } else {
        format!("({} {})", new_fn_name, free_vars.join(" "))
    };

    // Replace extracted range with the call.
    let mut edits = vec![Edit {
        byte_offset: start_byte,
        byte_len: end_byte - start_byte,
        replacement: format!("{}\n", replacement),
    }];

    let new_source = match apply_edits(&handle.source, &mut edits) {
        Ok(s) => s,
        Err(e) => {
            return (
                SIG_ERROR,
                ctx.error("rewrite-error", format!("compile/extract: {}", e)),
            )
        }
    };

    let capture_strs: Vec<Value> = free_vars.iter().map(|v| ctx.string(&**v)).collect();
    let captures_val = ctx.array(capture_strs);

    let mut fields = BTreeMap::new();
    let source_val = ctx.string(&*new_source);
    fields.insert(kw("source"), source_val);
    let new_function_val = ctx.string(&*new_function);
    fields.insert(kw("new-function"), new_function_val);
    fields.insert(kw("captures"), captures_val);
    let signal_val = signal_to_value(&signal, ctx);
    fields.insert(kw("signal"), signal_val);
    (SIG_OK, ctx.struct_from(fields))
}

/// (compile/parallelize analysis [:fn-a :fn-b]) → {:safe bool :reason "..." :code "..." :signal {...}}
pub(crate) fn prim_compile_parallelize(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/parallelize", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let fn_names: Vec<String> = match args[1].as_array() {
        Some(arr) => {
            let mut names = Vec::new();
            for v in arr {
                match v
                    .as_keyword_name()
                    .map(|s| s.to_string())
                    .or_else(|| v.with_string(|s| s.to_string()))
                {
                    Some(n) => names.push(n),
                    None => {
                        return (
                            SIG_ERROR,
                            ctx.error(
                                "type-error",
                                "compile/parallelize: names must be keywords or strings",
                            ),
                        )
                    }
                }
            }
            names
        }
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "compile/parallelize: second argument must be an array",
                ),
            )
        }
    };

    let symbols_ptr = ctx.vm().symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error("runtime-error", "compile/parallelize: no symbol table"),
        );
    }
    let symbols = unsafe { &*symbols_ptr };

    // Collect captures for each function.
    let mut all_captures: Vec<(String, Vec<(Binding, bool)>)> = Vec::new();
    let mut combined_signal = Signal::silent();

    for name in &fn_names {
        if let Some(sig) = handle.signal_map.get(name) {
            combined_signal = combined_signal.combine(*sig);
        }
        let mut caps = Vec::new();
        if let Some(lambda) = find_named_lambda(&handle.hir, &handle.arena, symbols, name) {
            if let HirKind::Lambda { captures, .. } = &lambda.kind {
                for cap in captures {
                    let inner = handle.arena.get(cap.binding);
                    caps.push((cap.binding, inner.is_mutated));
                }
            }
        }
        all_captures.push((name.clone(), caps));
    }

    // Check pairwise for shared mutable captures.
    let mut shared = Vec::new();
    for i in 0..all_captures.len() {
        for j in (i + 1)..all_captures.len() {
            for (b1, m1) in &all_captures[i].1 {
                for (b2, m2) in &all_captures[j].1 {
                    if b1 == b2 && (*m1 || *m2) {
                        if let Some(cap_name) = symbols.name(handle.arena.get(*b1).name) {
                            let mut f = BTreeMap::new();
                            f.insert(kw("name"), ctx.string(cap_name));
                            let kind = if handle.arena.get(*b1).needs_capture() {
                                "lbox"
                            } else {
                                "value"
                            };
                            f.insert(kw("kind"), Value::keyword(kind));
                            shared.push(ctx.struct_from(f));
                        }
                    }
                }
            }
        }
    }

    let safe = shared.is_empty();
    let mut fields = BTreeMap::new();
    fields.insert(kw("safe"), Value::bool(safe));

    if safe {
        let fn_list = fn_names.join(" ");
        let code = format!("(ev/map (fn [f] (f)) [{}])", fn_list);
        fields.insert(
            kw("reason"),
            ctx.string("No shared mutable captures between any pair."),
        );
        fields.insert(kw("code"), ctx.string(&*code));
    } else {
        fields.insert(
            kw("reason"),
            ctx.string("Shared mutable captures detected."),
        );
        fields.insert(kw("shared-captures"), ctx.array(shared));
    }

    let signal_val = signal_to_value(&combined_signal, ctx);
    fields.insert(kw("signal"), signal_val);
    (SIG_OK, ctx.struct_from(fields))
}
