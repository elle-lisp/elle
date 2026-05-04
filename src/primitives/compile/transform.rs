use std::collections::{BTreeMap, BTreeSet};

use crate::context;
use crate::hir::{Binding, HirKind};
use crate::rewrite::edit::{apply_edits, Edit};
use crate::signals::registry::with_registry;
use crate::signals::Signal;
use crate::value::error_val;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::sorted_struct_get;
use crate::value::Value;

use super::{
    collect_vars_in_range, compute_line_offsets, find_matching_paren, find_named_lambda,
    get_handle, kw, resolve_name, signal_to_value,
};

/// (compile/rename analysis :old-name :new-name) → {:source "..." :edits N}
pub(super) fn prim_compile_rename(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/rename") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let old_name = match resolve_name(args, 1, "compile/rename") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let new_name = match resolve_name(args, 2, "compile/rename") {
        Ok(n) => n,
        Err(e) => return e,
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/rename: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    // Find the first non-primitive binding matching old_name (file-scope first).
    let mut target_binding = None;
    for i in 0..handle.arena.len() {
        let binding = Binding(i as u32);
        let inner = handle.arena.get(binding);
        if let Some(name) = symbols.name(inner.name) {
            if name == old_name {
                target_binding = Some(binding);
                break;
            }
        }
    }
    let binding = match target_binding {
        Some(b) => b,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "lookup-error",
                    format!("compile/rename: no binding '{}' in analysis", old_name),
                ),
            )
        }
    };

    let name_spans = match handle.binding_spans.get(&binding) {
        Some(s) if !s.is_empty() => s,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "lookup-error",
                    format!("compile/rename: no source spans for '{}'", old_name),
                ),
            )
        }
    };

    let mut edits: Vec<Edit> = name_spans
        .iter()
        .map(|(offset, len)| Edit {
            byte_offset: *offset,
            byte_len: *len,
            replacement: new_name.clone(),
        })
        .collect();

    let count = edits.len();
    match apply_edits(&handle.source, &mut edits) {
        Ok(new_source) => {
            let mut fields = BTreeMap::new();
            fields.insert(kw("source"), Value::string(&*new_source));
            fields.insert(kw("edits"), Value::int(count as i64));
            (SIG_OK, Value::struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("rewrite-error", format!("compile/rename: {}", e)),
        ),
    }
}

/// (compile/extract analysis {:from :fn :lines [s e] :name :new}) → {:source :new-function :captures :signal}
pub(super) fn prim_compile_extract(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/extract") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let opts = match args[1].as_struct() {
        Some(f) => f,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
                error_val("type-error", "compile/extract: :from is required"),
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
                    error_val("type-error", "compile/extract: :lines must be [start end]"),
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
                error_val("type-error", "compile/extract: :name is required"),
            )
        }
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/extract: no symbol table"),
            )
        }
    };
    let symbols = unsafe { &*symbols_ptr };

    if start_line == 0 || end_line == 0 || start_line > end_line {
        return (
            SIG_ERROR,
            error_val("range-error", "compile/extract: invalid line range"),
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
                error_val(
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
                error_val("rewrite-error", format!("compile/extract: {}", e)),
            )
        }
    };

    let captures_val = Value::array(free_vars.iter().map(|v| Value::string(&**v)).collect());

    let mut fields = BTreeMap::new();
    fields.insert(kw("source"), Value::string(&*new_source));
    fields.insert(kw("new-function"), Value::string(&*new_function));
    fields.insert(kw("captures"), captures_val);
    fields.insert(kw("signal"), signal_to_value(&signal));
    (SIG_OK, Value::struct_from(fields))
}

/// (compile/parallelize analysis [:fn-a :fn-b]) → {:safe bool :reason "..." :code "..." :signal {...}}
pub(super) fn prim_compile_parallelize(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/parallelize") {
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
                            error_val(
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
                error_val(
                    "type-error",
                    "compile/parallelize: second argument must be an array",
                ),
            )
        }
    };

    let symbols_ptr = match unsafe { context::get_symbol_table() } {
        Some(ptr) => ptr,
        None => {
            return (
                SIG_ERROR,
                error_val("runtime-error", "compile/parallelize: no symbol table"),
            )
        }
    };
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
                            f.insert(kw("name"), Value::string(cap_name));
                            let kind = if handle.arena.get(*b1).needs_capture() {
                                "lbox"
                            } else {
                                "value"
                            };
                            f.insert(kw("kind"), Value::keyword(kind));
                            shared.push(Value::struct_from(f));
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
            Value::string("No shared mutable captures between any pair."),
        );
        fields.insert(kw("code"), Value::string(&*code));
    } else {
        fields.insert(
            kw("reason"),
            Value::string("Shared mutable captures detected."),
        );
        fields.insert(kw("shared-captures"), Value::array(shared));
    }

    fields.insert(kw("signal"), signal_to_value(&combined_signal));
    (SIG_OK, Value::struct_from(fields))
}

/// (compile/add-handler analysis :fn-name :signal-kind) → {:source "..." :wraps N}
pub(super) fn prim_compile_add_handler(args: &[Value]) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/add-handler") {
        Ok(h) => h,
        Err(e) => return e,
    };
    let fn_name = match resolve_name(args, 1, "compile/add-handler") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let signal_kind = match resolve_name(args, 2, "compile/add-handler") {
        Ok(n) => n,
        Err(e) => return e,
    };

    // Verify the function emits the signal.
    let sig = match handle.signal_map.get(&fn_name) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "lookup-error",
                    format!("compile/add-handler: no function '{}'", fn_name),
                ),
            )
        }
    };

    let bit = match with_registry(|reg| reg.lookup(&signal_kind)) {
        Some(b) => b,
        None => match signal_kind.as_str() {
            "error" => 0,
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "lookup-error",
                        format!("compile/add-handler: unknown signal '{}'", signal_kind),
                    ),
                )
            }
        },
    };

    if !sig.bits.has_bit(bit) && sig.propagates & (1 << bit) == 0 {
        return (
            SIG_ERROR,
            error_val(
                "signal-error",
                format!(
                    "compile/add-handler: '{}' does not emit :{}",
                    fn_name, signal_kind
                ),
            ),
        );
    }

    // Find call sites via reverse call graph.
    let callers = handle
        .call_graph
        .reverse
        .get(&fn_name)
        .cloned()
        .unwrap_or_default();

    let line_offsets = compute_line_offsets(&handle.source);
    let mut edits = Vec::new();

    for caller_name in &callers {
        if let Some(edges) = handle.call_graph.edges.get(caller_name) {
            for edge in edges {
                if edge.callee == fn_name {
                    if let Some(&line_start) =
                        line_offsets.get((edge.line.saturating_sub(1)) as usize)
                    {
                        let byte_offset = line_start + (edge.col.saturating_sub(1)) as usize;
                        if let Some(call_end) = find_matching_paren(&handle.source, byte_offset) {
                            let call_text = &handle.source[byte_offset..call_end];
                            let wrapped = match signal_kind.as_str() {
                                "error" => format!(
                                    "(let [[ok? result] (protect {})] \
                                     (if ok? result (begin (eprintln \"error:\" result) nil)))",
                                    call_text
                                ),
                                "io" => format!("(with-timeout 5000 {})", call_text),
                                _ => format!("(protect {})", call_text),
                            };
                            edits.push(Edit {
                                byte_offset,
                                byte_len: call_end - byte_offset,
                                replacement: wrapped,
                            });
                        }
                    }
                }
            }
        }
    }

    let wrap_count = edits.len() as i64;
    match apply_edits(&handle.source, &mut edits) {
        Ok(new_source) => {
            let mut fields = BTreeMap::new();
            fields.insert(kw("source"), Value::string(&*new_source));
            fields.insert(kw("wraps"), Value::int(wrap_count));
            (SIG_OK, Value::struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("rewrite-error", format!("compile/add-handler: {}", e)),
        ),
    }
}

// ── compile/run-on ─────────────────────────────────────────────────────

/// `(compile/run-on tier f & args)` — force-dispatch `f` on the named tier.
///
/// Powers `lib/differential.lisp`. Returns the result, or signals
/// `:tier-rejected` if the tier doesn't accept this closure.
///
/// Tiers: `:bytecode`, `:jit`, `:mlir-cpu` (the last requires `--features mlir`).
///
/// Implementation: returns `SIG_QUERY` with payload `(tier closure arg1 arg2 ...)`;
/// the VM's `dispatch_compile_run_on` handler does the actual work because it
/// needs `&mut VM` access for the JIT cache, MLIR cache, and call machinery.
pub(super) fn prim_compile_run_on(args: &[Value]) -> (SignalBits, Value) {
    // Cheap front-end validation — full type checks happen in the dispatch handler.
    if args[0].as_keyword_name().is_none() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "compile/run-on: tier must be a keyword, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    if args[1].as_closure().is_none() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "compile/run-on: target must be a closure, got {}",
                    args[1].type_name()
                ),
            ),
        );
    }

    // Forward the entire arg list to the VM dispatcher.
    (
        SIG_QUERY,
        Value::pair(
            Value::keyword("compile/run-on"),
            crate::value::list(args.to_vec()),
        ),
    )
}
