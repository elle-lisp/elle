use super::*;

/// Check if a byte can appear in an Elle identifier token.
pub(crate) fn is_ident_byte(b: u8) -> bool {
    !b.is_ascii_whitespace()
        && !matches!(
            b,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'#' | b'"' | b'\''
        )
}
/// Find the first occurrence of `name` as a standalone token in `source[start..end]`.
/// Returns `(absolute_byte_offset, byte_len)`.
pub(crate) fn find_name_in_span(
    source: &str,
    start: usize,
    end: usize,
    name: &str,
) -> Option<NameSpan> {
    if start >= end || name.is_empty() || end > source.len() || start > source.len() {
        return None;
    }
    // Clamp to valid source range.
    let end = end.min(source.len());
    if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return None;
    }
    let region = &source.as_bytes()[start..end];
    let nb = name.as_bytes();
    let nlen = nb.len();
    if nlen > region.len() {
        return None;
    }
    for i in 0..=(region.len() - nlen) {
        if &region[i..i + nlen] == nb {
            let before_ok = i == 0 || !is_ident_byte(region[i - 1]);
            let after_ok = i + nlen >= region.len() || !is_ident_byte(region[i + nlen]);
            if before_ok && after_ok {
                return Some((start + i, nlen));
            }
        }
    }
    None
}
/// Select the binding named `name` that actually carries symbol-index data (a
/// definition location or usages).
///
/// A file-scope `def` can leave a phantom prebind binding that shares the name
/// but holds no source info (the analyzer's two-pass file-letrec). The index is
/// keyed per-binding (`DefId`) now, so a by-name reflection lookup must skip
/// such phantoms or it lands on an empty entry. Falls back to the first by-name
/// binding when none carry data, so a name with no span data still resolves.
pub(crate) fn binding_for_name(
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    index: &SymbolIndex,
    name: &str,
) -> Option<Binding> {
    let mut fallback = None;
    for i in 0..arena.len() {
        let b = Binding(i as u32);
        if symbols.name(arena.get(b).name) == Some(name) {
            let id = b.def_id();
            if index.symbol_locations.contains_key(&id) || index.symbol_usages.contains_key(&id) {
                return Some(b);
            }
            if fallback.is_none() {
                fallback = Some(b);
            }
        }
    }
    fallback
}

/// Build binding spans using symbol_index data (line/col → byte offsets).
/// HIR byte spans are unreliable for macro-expanded code, so we use the
/// symbol_index which records definition and usage locations correctly.
pub(crate) fn build_binding_spans(
    _hir: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    source: &str,
    symbol_index: &SymbolIndex,
    spans: &mut HashMap<Binding, Vec<NameSpan>>,
) {
    let line_offsets = compute_line_offsets(source);

    for i in 0..arena.len() {
        let binding = Binding(i as u32);
        let inner = arena.get(binding);
        let name = match symbols.name(inner.name) {
            Some(n) => n,
            None => continue,
        };

        // Definition site from symbol_locations. Keyed per-binding (DefId), so
        // two distinct bindings that share a name read independent locations —
        // the arena index here aligns with the index's DefIds.
        if let Some(loc) = symbol_index.symbol_locations.get(&binding.def_id()) {
            if loc.line > 0 {
                if let Some(&line_start) = line_offsets.get(loc.line - 1) {
                    let byte_start = line_start + loc.col.saturating_sub(1);
                    if byte_start < source.len() {
                        let search_end = (byte_start + name.len() + 20).min(source.len());
                        if let Some(ns) = find_name_in_span(source, byte_start, search_end, name) {
                            spans.entry(binding).or_default().push(ns);
                        }
                    }
                }
            }
        }

        // Usage sites from symbol_usages (deduplicated against definition site).
        if let Some(usages) = symbol_index.symbol_usages.get(&binding.def_id()) {
            for usage in usages {
                if usage.line > 0 {
                    if let Some(&line_start) = line_offsets.get(usage.line - 1) {
                        let byte_start = line_start + usage.col.saturating_sub(1);
                        if byte_start >= source.len() {
                            continue;
                        }
                        let search_end = (byte_start + name.len() + 10).min(source.len());
                        if let Some(ns) = find_name_in_span(source, byte_start, search_end, name) {
                            let entry = spans.entry(binding).or_default();
                            if !entry.contains(&ns) {
                                entry.push(ns);
                            }
                        }
                    }
                }
            }
        }
    }
}
/// Find the Lambda Hir node for a named function definition.
pub(crate) fn find_named_lambda<'a>(
    hir: &'a Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    target: &str,
) -> Option<&'a Hir> {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let Some(name) = symbols.name(arena.get(*binding).name) {
                    if name == target && matches!(value.kind, HirKind::Lambda { .. }) {
                        return Some(value);
                    }
                }
                if let Some(r) = find_named_lambda(value, arena, symbols, target) {
                    return Some(r);
                }
            }
            find_named_lambda(body, arena, symbols, target)
        }
        HirKind::Define { binding, value } => {
            if let Some(name) = symbols.name(arena.get(*binding).name) {
                if name == target && matches!(value.kind, HirKind::Lambda { .. }) {
                    return Some(value);
                }
            }
            find_named_lambda(value, arena, symbols, target)
        }
        HirKind::Lambda { body, .. } => find_named_lambda(body, arena, symbols, target),
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => find_named_lambda(cond, arena, symbols, target)
            .or_else(|| find_named_lambda(then_branch, arena, symbols, target))
            .or_else(|| find_named_lambda(else_branch, arena, symbols, target)),
        HirKind::Begin(es) | HirKind::And(es) | HirKind::Or(es) => {
            for e in es {
                if let Some(r) = find_named_lambda(e, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Block { body, .. } => {
            for e in body {
                if let Some(r) = find_named_lambda(e, arena, symbols, target) {
                    return Some(r);
                }
            }
            None
        }
        HirKind::Call { func, args, .. } => find_named_lambda(func, arena, symbols, target)
            .or_else(|| {
                for a in args {
                    if let Some(r) = find_named_lambda(&a.expr, arena, symbols, target) {
                        return Some(r);
                    }
                }
                None
            }),
        HirKind::Assign { value, .. } => find_named_lambda(value, arena, symbols, target),
        HirKind::While { cond, body } => find_named_lambda(cond, arena, symbols, target)
            .or_else(|| find_named_lambda(body, arena, symbols, target)),
        HirKind::Match { value, arms } => {
            find_named_lambda(value, arena, symbols, target).or_else(|| {
                for (_, g, b) in arms {
                    if let Some(g) = g {
                        if let Some(r) = find_named_lambda(g, arena, symbols, target) {
                            return Some(r);
                        }
                    }
                    if let Some(r) = find_named_lambda(b, arena, symbols, target) {
                        return Some(r);
                    }
                }
                None
            })
        }
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
            find_named_lambda(e, arena, symbols, target)
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                if let Some(r) = find_named_lambda(c, arena, symbols, target) {
                    return Some(r);
                }
                if let Some(r) = find_named_lambda(b, arena, symbols, target) {
                    return Some(r);
                }
            }
            else_branch
                .as_ref()
                .and_then(|e| find_named_lambda(e, arena, symbols, target))
        }
        HirKind::Destructure { value, .. } => find_named_lambda(value, arena, symbols, target),
        HirKind::Eval { expr, env } => find_named_lambda(expr, arena, symbols, target)
            .or_else(|| find_named_lambda(env, arena, symbols, target)),
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                if let Some(r) = find_named_lambda(p, arena, symbols, target) {
                    return Some(r);
                }
                if let Some(r) = find_named_lambda(v, arena, symbols, target) {
                    return Some(r);
                }
            }
            find_named_lambda(body, arena, symbols, target)
        }
        _ => None,
    }
}
/// Collect referenced and defined bindings within a byte range of the HIR.
pub(crate) fn collect_vars_in_range(
    hir: &Hir,
    start: usize,
    end: usize,
    referenced: &mut BTreeSet<Binding>,
    defined: &mut BTreeSet<Binding>,
    signal: &mut Signal,
) {
    if hir.span.start as usize >= end || (hir.span.end as usize) <= start {
        return;
    }
    *signal = signal.combine(hir.signal);
    match &hir.kind {
        HirKind::Var(b) => {
            referenced.insert(*b);
        }
        HirKind::Define { binding, value } => {
            defined.insert(*binding);
            collect_vars_in_range(value, start, end, referenced, defined, signal);
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (b, init) in bindings {
                defined.insert(*b);
                collect_vars_in_range(init, start, end, referenced, defined, signal);
            }
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        HirKind::Lambda {
            params,
            rest_param,
            body,
            ..
        } => {
            for p in params {
                defined.insert(*p);
            }
            if let Some(r) = rest_param {
                defined.insert(*r);
            }
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        HirKind::Assign { target, value } => {
            referenced.insert(*target);
            collect_vars_in_range(value, start, end, referenced, defined, signal);
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_vars_in_range(cond, start, end, referenced, defined, signal);
            collect_vars_in_range(then_branch, start, end, referenced, defined, signal);
            collect_vars_in_range(else_branch, start, end, referenced, defined, signal);
        }
        HirKind::Begin(es) | HirKind::And(es) | HirKind::Or(es) => {
            for e in es {
                collect_vars_in_range(e, start, end, referenced, defined, signal);
            }
        }
        HirKind::Block { body, .. } => {
            for e in body {
                collect_vars_in_range(e, start, end, referenced, defined, signal);
            }
        }
        HirKind::Call { func, args, .. } => {
            collect_vars_in_range(func, start, end, referenced, defined, signal);
            for a in args {
                collect_vars_in_range(&a.expr, start, end, referenced, defined, signal);
            }
        }
        HirKind::While { cond, body } => {
            collect_vars_in_range(cond, start, end, referenced, defined, signal);
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        HirKind::Match { value, arms } => {
            collect_vars_in_range(value, start, end, referenced, defined, signal);
            for (_, g, b) in arms {
                if let Some(g) = g {
                    collect_vars_in_range(g, start, end, referenced, defined, signal);
                }
                collect_vars_in_range(b, start, end, referenced, defined, signal);
            }
        }
        HirKind::Emit { value: e, .. } | HirKind::Break { value: e, .. } => {
            collect_vars_in_range(e, start, end, referenced, defined, signal);
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            for (c, b) in clauses {
                collect_vars_in_range(c, start, end, referenced, defined, signal);
                collect_vars_in_range(b, start, end, referenced, defined, signal);
            }
            if let Some(e) = else_branch {
                collect_vars_in_range(e, start, end, referenced, defined, signal);
            }
        }
        HirKind::Destructure { value, .. } => {
            collect_vars_in_range(value, start, end, referenced, defined, signal);
        }
        HirKind::Eval { expr, env } => {
            collect_vars_in_range(expr, start, end, referenced, defined, signal);
            collect_vars_in_range(env, start, end, referenced, defined, signal);
        }
        HirKind::Parameterize { bindings, body } => {
            for (p, v) in bindings {
                collect_vars_in_range(p, start, end, referenced, defined, signal);
                collect_vars_in_range(v, start, end, referenced, defined, signal);
            }
            collect_vars_in_range(body, start, end, referenced, defined, signal);
        }
        _ => {}
    }
}
/// Compute byte offsets for each line start (0-indexed lines).
pub(crate) fn compute_line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            offsets.push(i + 1);
        }
    }
    offsets
}
/// Find the matching close paren for an open paren at `start`.
pub(crate) fn find_matching_paren(source: &str, start: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if start >= bytes.len() || bytes[start] != b'(' {
        return None;
    }
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'"' if !in_string => in_string = true,
            b'"' if in_string => in_string = false,
            b'\\' if in_string => {
                i += 1;
            }
            b'(' if !in_string => depth += 1,
            b')' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}
