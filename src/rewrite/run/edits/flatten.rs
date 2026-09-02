//! Flatten, bracket, and clause-flatten edit collectors.

use super::*;

/// Lex source and collect edits that flatten nested-pair binding vectors.
/// Matches `( let|letrec [ [p1 v1] [p2 v2] ... ] body... )` and deletes
/// the inner `[`/`]` (or `(`/`)`) delimiters, leaving the contents flat.
pub(crate) fn collect_flatten_edits(
    src: SourceText<'_>,
    flatten_syms: &[&str],
) -> Result<Vec<Edit>, String> {
    let tokens = src.code_tokens()?;

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for `(` symbol `[`  where symbol is in flatten_syms
        if matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
            if let Some((Token::Symbol(s), _, _)) = tokens.get(i + 1) {
                if flatten_syms.contains(s) {
                    if let Some(new_edits) = try_match_flatten(src.text, &tokens, i) {
                        edits.extend(new_edits);
                        // Don't skip the whole form — advance past `(` and symbol
                        // so nested let/letrec forms in the body are still visited.
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    Ok(edits)
}

/// Try to flatten the bindings vector of a let/letrec form at token `i`.
/// Returns edits that delete the inner pair delimiters if the form matches
/// the nested-pair pattern.
pub(super) fn try_match_flatten(
    _source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    i: usize,
) -> Option<Vec<Edit>> {
    // tokens[i] = `(`, tokens[i+1] = let/letrec, tokens[i+2] should be `[` or `(`
    let bindings_open = i + 2;
    if bindings_open >= tokens.len() {
        return None;
    }
    let open_token = &tokens[bindings_open].0;
    if !matches!(open_token, Token::LeftBracket | Token::LeftParen) {
        return None;
    }

    // Find the matching close of the bindings container
    let bindings_close = skip_balanced_form(tokens, bindings_open);
    if bindings_close == 0 {
        return None;
    }
    let close_idx = bindings_close - 1; // index of the `]` or `)` token

    // Walk direct children of the bindings container.
    // Each child must be a 2-element list/array (the nested-pair format).
    // If any child is an atom, it's already flat — skip.
    let mut pairs: Vec<(usize, usize)> = Vec::new(); // (open_idx, close_idx) for each inner pair
    let mut pos = bindings_open + 1; // skip the opening `[` of bindings
    while pos < close_idx {
        match &tokens[pos].0 {
            Token::LeftBracket | Token::LeftParen => {
                let pair_open = pos;
                let pair_close_next = skip_balanced_form(tokens, pos);
                if pair_close_next == 0 {
                    return None;
                }
                let pair_close = pair_close_next - 1;

                // Count the children of this inner form to verify it has exactly 2
                let mut child_count = 0;
                let mut child_pos = pair_open + 1;
                while child_pos < pair_close {
                    child_count += 1;
                    child_pos = skip_one_form(tokens, child_pos);
                }

                if child_count != 2 {
                    // Not a 2-element pair — this might be a destructuring pattern
                    // in an already-flat binding. Skip this form entirely.
                    return None;
                }

                pairs.push((pair_open, pair_close));
                pos = pair_close_next;
            }
            _ => {
                // Atom found at top level of bindings — already flat
                return None;
            }
        }
    }

    if pairs.is_empty() {
        return None;
    }

    // Generate edits: for each inner pair, delete the opening and closing delimiters.
    // We need to handle whitespace carefully — consume trailing whitespace after the
    // opening delimiter and leading whitespace before the closing delimiter.
    let mut edits = Vec::new();
    for &(open_idx, close_idx) in &pairs {
        let open_byte = tokens[open_idx].1;
        // Delete the opening delimiter. Also consume any whitespace between it and
        // the first child form.
        let next_byte = tokens[open_idx + 1].1;
        edits.push(Edit {
            byte_offset: open_byte,
            byte_len: next_byte - open_byte,
            replacement: String::new(),
        });

        // Delete the closing delimiter. Also consume whitespace before it.
        let close_byte = tokens[close_idx].1;
        let close_len = tokens[close_idx].2;
        let prev_end_idx = close_idx - 1;
        let prev_end = tokens[prev_end_idx].1 + tokens[prev_end_idx].2;
        edits.push(Edit {
            byte_offset: prev_end,
            byte_len: close_byte + close_len - prev_end,
            replacement: String::new(),
        });
    }

    Some(edits)
}

/// Normalize paren-delimited binding vectors to brackets.
/// Matches `(let|letrec|let*|if-let|when-let|when-ok (bindings...) body...)`
/// where the bindings container uses `(...)` and replaces with `[...]`.
pub(crate) fn collect_bracket_edits(
    src: SourceText<'_>,
    binding_forms: &[&str],
) -> Result<Vec<Edit>, String> {
    let tokens = src.code_tokens()?;

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for `( symbol (` where symbol is a binding form and the
        // bindings container uses parens instead of brackets.
        if matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
            if let Some((Token::Symbol(s), _, _)) = tokens.get(i + 1) {
                if binding_forms.contains(s) {
                    if let Some((Token::LeftParen, open_byte, open_len)) = tokens.get(i + 2) {
                        // Find the matching close paren
                        let close_next = skip_balanced_form(&tokens, i + 2);
                        if close_next > 0 {
                            let close_idx = close_next - 1;
                            let (_, close_byte, close_len) = tokens[close_idx];
                            // Replace `(` with `[` and `)` with `]`
                            edits.push(Edit {
                                byte_offset: *open_byte,
                                byte_len: *open_len,
                                replacement: "[".to_string(),
                            });
                            edits.push(Edit {
                                byte_offset: close_byte,
                                byte_len: close_len,
                                replacement: "]".to_string(),
                            });
                        }
                    }
                }
            }
        }
        i += 1;
    }
    Ok(edits)
}

/// Lex source and collect edits that flatten parenthesized cond/match clauses.
/// Matches `(cond (test body) ...)` or `(match val (pat body) ...)` and
/// removes the inner clause delimiters, wrapping multi-body arms in `(begin ...)`.
pub(crate) fn collect_flatten_clause_edits(
    src: SourceText<'_>,
    flatten_clauses: &[(&str, usize)],
) -> Result<Vec<Edit>, String> {
    // Comments stay in this stream: a clause walk that counts them as
    // children is what the pinned rewrites of `cond` and `match` expect.
    let tokens: Vec<(Token<'_>, usize, usize)> = src
        .tokens()?
        .into_iter()
        .map(|t| (t.token, t.byte_offset, t.len))
        .collect();

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for `(` symbol where symbol is in flatten_clauses
        if matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
            if let Some((Token::Symbol(s), _, _)) = tokens.get(i + 1) {
                if let Some(&(_, skip)) = flatten_clauses.iter().find(|(sym, _)| sym == s) {
                    if let Some(new_edits) = try_match_flatten_clauses(src.text, &tokens, i, skip) {
                        edits.extend(new_edits);
                        // Don't skip the whole form — advance past head symbol
                        // so nested forms in the body are still visited.
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    Ok(edits)
}

/// Try to flatten the clauses of a cond/match form at token `i`.
/// `skip` is 0 for cond (no args before clauses) or 1 for match (skip value expr).
pub(super) fn try_match_flatten_clauses(
    source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    i: usize,
    skip: usize,
) -> Option<Vec<Edit>> {
    // tokens[i] = `(`, tokens[i+1] = cond/match
    // Skip past head symbol + `skip` argument forms
    let mut pos = i + 2; // after `(` and symbol
    for _ in 0..skip {
        if pos >= tokens.len() {
            return None;
        }
        pos = skip_one_form(tokens, pos);
    }

    // Find the closing paren of the outer form
    let outer_close = skip_balanced_form(tokens, i);
    if outer_close == 0 {
        return None;
    }
    let outer_close_idx = outer_close - 1;

    // Walk remaining children — each should be a parenthesized clause
    let mut edits = Vec::new();
    let mut any_clause = false;
    while pos < outer_close_idx {
        match &tokens[pos].0 {
            Token::LeftParen | Token::LeftBracket => {
                let clause_open = pos;
                let clause_close_next = skip_balanced_form(tokens, pos);
                if clause_close_next == 0 {
                    return None;
                }
                let clause_close = clause_close_next - 1;

                // Count children and find their positions
                let mut children: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte)
                let mut child_pos = clause_open + 1;
                while child_pos < clause_close {
                    let child_start = tokens[child_pos].1;
                    let child_end_pos = skip_one_form(tokens, child_pos);
                    let last = child_end_pos - 1;
                    let child_end = tokens[last].1 + tokens[last].2;
                    children.push((child_start, child_end));
                    child_pos = child_end_pos;
                }

                if children.is_empty() {
                    pos = clause_close_next;
                    continue;
                }

                // Check for (else body) in cond — replace with just body
                let first_text = &source[children[0].0..children[0].1];
                if first_text == "else" && children.len() >= 2 {
                    // Replace entire clause with just the body part(s)
                    let clause_start = tokens[clause_open].1;
                    let clause_end = tokens[clause_close].1 + tokens[clause_close].2;
                    if children.len() == 2 {
                        let body_text = &source[children[1].0..children[1].1];
                        edits.push(Edit {
                            byte_offset: clause_start,
                            byte_len: clause_end - clause_start,
                            replacement: body_text.to_string(),
                        });
                    } else {
                        // Multi-body else: wrap in (begin ...)
                        let body_parts: Vec<&str> =
                            children[1..].iter().map(|(s, e)| &source[*s..*e]).collect();
                        edits.push(Edit {
                            byte_offset: clause_start,
                            byte_len: clause_end - clause_start,
                            replacement: format!("(begin {})", body_parts.join(" ")),
                        });
                    }
                    any_clause = true;
                    pos = clause_close_next;
                    continue;
                }

                // Normal clause: delete delimiters
                if children.len() == 2 {
                    // Simple 2-element clause: just remove the outer parens
                    let open_byte = tokens[clause_open].1;
                    let next_byte = children[0].0;
                    edits.push(Edit {
                        byte_offset: open_byte,
                        byte_len: next_byte - open_byte,
                        replacement: String::new(),
                    });
                    let close_byte = tokens[clause_close].1;
                    let close_len = tokens[clause_close].2;
                    let prev_end = children[children.len() - 1].1;
                    edits.push(Edit {
                        byte_offset: prev_end,
                        byte_len: close_byte + close_len - prev_end,
                        replacement: String::new(),
                    });
                    any_clause = true;
                } else if children.len() >= 3 {
                    // Check for guard pattern: (pat when guard body...)
                    let second_text = &source[children[1].0..children[1].1];
                    if second_text == "when" && children.len() >= 4 {
                        // Guard: just remove outer parens (all elements stay flat)
                        let open_byte = tokens[clause_open].1;
                        let next_byte = children[0].0;
                        edits.push(Edit {
                            byte_offset: open_byte,
                            byte_len: next_byte - open_byte,
                            replacement: String::new(),
                        });
                        let close_byte = tokens[clause_close].1;
                        let close_len = tokens[clause_close].2;
                        let prev_end = children[children.len() - 1].1;
                        edits.push(Edit {
                            byte_offset: prev_end,
                            byte_len: close_byte + close_len - prev_end,
                            replacement: String::new(),
                        });
                    } else {
                        // Multi-body: pattern + (begin body...)
                        let clause_start = tokens[clause_open].1;
                        let clause_end = tokens[clause_close].1 + tokens[clause_close].2;
                        let pattern_text = &source[children[0].0..children[0].1];
                        let body_parts: Vec<&str> =
                            children[1..].iter().map(|(s, e)| &source[*s..*e]).collect();
                        edits.push(Edit {
                            byte_offset: clause_start,
                            byte_len: clause_end - clause_start,
                            replacement: format!(
                                "{} (begin {})",
                                pattern_text,
                                body_parts.join(" ")
                            ),
                        });
                    }
                    any_clause = true;
                } else {
                    // Single-element clause — pass through
                    pos = clause_close_next;
                    continue;
                }

                pos = clause_close_next;
            }
            _ => {
                // Not a parenthesized clause — already flat or atom
                // Don't treat this as needing flattening
                return None;
            }
        }
    }

    if any_clause {
        Some(edits)
    } else {
        None
    }
}
