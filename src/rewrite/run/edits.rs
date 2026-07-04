use super::*;

/// Scan source for removed symbols and return an error listing them.
pub(super) fn check_removals(
    source: &str,
    removals: &HashMap<&str, &str>,
    file_path: &str,
) -> Result<(), String> {
    let tokens = lex_tokens_no_comments(source)?;

    let mut errors = Vec::new();
    for (token, _, _) in &tokens {
        if let Token::Symbol(name) = token {
            if let Some(msg) = removals.get(*name) {
                errors.push(format!("  `{}` has been removed — {}", name, msg));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}: removed symbols found:\n{}",
            file_path,
            errors.join("\n")
        ))
    }
}

/// Lex source and collect edits for forms matching unwrap rules.
/// Matches `(symbol (fn [] body...))` or `(symbol (fn () body...))` and
/// replaces the entire form with just the body.
pub(super) fn collect_unwrap_edits(
    source: &str,
    unwraps: &HashMap<&str, &str>,
    file_path: &str,
) -> Result<Vec<Edit>, String> {
    let tokens = lex_tokens_no_comments(source)?;

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(edit) = try_match_unwrap(source, &tokens, i, unwraps) {
            i = skip_balanced_form(&tokens, i);
            edits.push(edit);
        } else {
            // Check for non-unwrappable uses (ev/run with wrong pattern)
            if let Token::Symbol(name) = &tokens[i].0 {
                if let Some(msg) = unwraps.get(*name) {
                    // Check if this is in head position of a list
                    if i > 0 && matches!(tokens[i - 1].0, Token::LeftParen) {
                        return Err(format!(
                            "{}: `{}` cannot be automatically unwrapped — {}",
                            file_path, name, msg
                        ));
                    }
                }
            }
            i += 1;
        }
    }
    Ok(edits)
}

/// Try to match an unwrap rule: `(symbol (fn [] body...))` → `body...`
pub(super) fn try_match_unwrap<'a>(
    source: &str,
    tokens: &[(Token<'a>, usize, usize)],
    i: usize,
    unwraps: &HashMap<&str, &str>,
) -> Option<Edit> {
    // Must be `(` symbol `(` fn `[]` or `()` ...body... `)` `)`
    if !matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
        return None;
    }
    let head_sym = match tokens.get(i + 1) {
        Some((Token::Symbol(s), _, _)) => *s,
        _ => return None,
    };
    if !unwraps.contains_key(head_sym) {
        return None;
    }
    // Next must be `(` fn
    if !matches!(tokens.get(i + 2), Some((Token::LeftParen, _, _))) {
        return None;
    }
    if !matches!(tokens.get(i + 3), Some((Token::Symbol(s), _, _)) if *s == "fn") {
        return None;
    }
    // Next must be `[]` or `()`
    let params_start = i + 4;
    let params_end = match tokens.get(params_start) {
        Some((Token::LeftBracket, _, _)) => {
            // Check for empty brackets: [ ]
            if matches!(
                tokens.get(params_start + 1),
                Some((Token::RightBracket, _, _))
            ) {
                params_start + 2
            } else {
                return None; // non-empty params
            }
        }
        Some((Token::LeftParen, _, _)) => {
            // Check for empty parens: ( )
            if matches!(
                tokens.get(params_start + 1),
                Some((Token::RightParen, _, _))
            ) {
                params_start + 2
            } else {
                return None; // non-empty params
            }
        }
        _ => return None,
    };

    // Body starts at params_end, ends before the inner `)` of `(fn [] body...)`
    // then the outer `)` of `(ev/run ...)`
    // Find the body text: from first body token to before inner `)`
    let body_start_byte = tokens.get(params_end).map(|t| t.1)?;

    // Find the matching `)` for the `(fn` — walk balanced from i+2
    let inner_close = skip_balanced_form(tokens, i + 2);
    if inner_close == 0 {
        return None;
    }
    let inner_close_idx = inner_close - 1; // index of the `)` token

    // Body ends before this `)`
    let body_end_byte = tokens.get(inner_close_idx).map(|t| t.1)?;

    // The outer form spans from `(` at i to `)` after the inner close
    let outer_close = skip_balanced_form(tokens, i);
    let form_start = tokens[i].1;
    let form_end = tokens.get(outer_close - 1).map(|t| t.1 + t.2)?;

    let body_text = source[body_start_byte..body_end_byte].trim();

    Some(Edit {
        byte_offset: form_start,
        byte_len: form_end - form_start,
        replacement: body_text.to_string(),
    })
}

/// Lex source and collect edits for forms matching replace rules.
/// Works at the token level using byte offsets from the lexer.
pub(super) fn collect_replace_edits(
    source: &str,
    replaces: &[(&str, usize, &str)],
) -> Result<Vec<Edit>, String> {
    let tokens = lex_tokens_no_comments(source)?;

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(edit) = try_match_replace(source, &tokens, i, replaces) {
            // Skip past the matched form
            i = skip_balanced_form(&tokens, i);
            edits.push(edit);
        } else {
            i += 1;
        }
    }
    Ok(edits)
}

/// Try to match a replace rule at token position `i`.
/// Expects `tokens[i]` to be `LeftParen` followed by a matching symbol.
pub(super) fn try_match_replace<'a>(
    source: &str,
    tokens: &[(Token<'a>, usize, usize)],
    i: usize,
    replaces: &[(&str, usize, &str)],
) -> Option<Edit> {
    // Must start with LeftParen
    if !matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
        return None;
    }
    // Next token must be a symbol matching a replace rule
    let head_sym = match tokens.get(i + 1) {
        Some((Token::Symbol(s), _, _)) => *s,
        _ => return None,
    };
    let (_, arity, template) = replaces.iter().find(|(s, _, _)| *s == head_sym)?;

    // Collect argument byte ranges by walking balanced tokens
    let mut args: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte) per arg
    let mut pos = i + 2; // skip LeftParen and head symbol
    while pos < tokens.len() {
        match &tokens[pos].0 {
            Token::RightParen => break,
            _ => {
                let arg_start = tokens[pos].1;
                let arg_end_pos = skip_one_form(tokens, pos);
                if arg_end_pos == 0 || arg_end_pos > tokens.len() {
                    return None; // malformed
                }
                let last = arg_end_pos - 1;
                let arg_end = tokens[last].1 + tokens[last].2;
                args.push((arg_start, arg_end));
                pos = arg_end_pos;
            }
        }
    }

    if pos >= tokens.len() || args.len() != *arity {
        return None;
    }

    // Form spans from LeftParen to RightParen (inclusive)
    let form_start = tokens[i].1;
    let form_end = tokens[pos].1 + tokens[pos].2; // byte after )

    // Build replacement by interpolating source text of args into template
    let mut result = template.to_string();
    for (j, (start, end)) in args.iter().enumerate().rev() {
        let placeholder = format!("${}", j + 1);
        result = result.replace(&placeholder, &source[*start..*end]);
    }

    Some(Edit {
        byte_offset: form_start,
        byte_len: form_end - form_start,
        replacement: result,
    })
}

/// Skip past one balanced form starting at `pos`. Returns the index after the form.
pub(super) fn skip_one_form(tokens: &[(Token<'_>, usize, usize)], pos: usize) -> usize {
    match &tokens[pos].0 {
        Token::LeftParen | Token::LeftBracket | Token::LeftBrace => skip_balanced_form(tokens, pos),
        // |...| set literal — scan to matching |
        Token::Pipe => skip_pipe_form(tokens, pos),
        // Prefix tokens: skip the prefix then the following form
        Token::Quote
        | Token::Quasiquote
        | Token::Unquote
        | Token::UnquoteSplicing
        | Token::Splice => skip_one_form(tokens, pos + 1),
        // @[...], @{...} — prefix then balanced form
        Token::ListSugar => skip_one_form(tokens, pos + 1),
        // @|...| — scan for closing |
        Token::AtPipe => skip_pipe_form(tokens, pos),
        _ => pos + 1, // atom: single token
    }
}

/// Skip a balanced delimited form (list/array/struct) starting at `pos`.
/// Returns the index after the closing delimiter.
pub(super) fn skip_balanced_form(tokens: &[(Token<'_>, usize, usize)], start: usize) -> usize {
    let mut depth = 0i32;
    let mut pos = start;
    while pos < tokens.len() {
        match &tokens[pos].0 {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth -= 1;
                if depth == 0 {
                    return pos + 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    pos
}

/// Skip a `|...|` set literal. Scan for the matching closing `|`.
pub(super) fn skip_pipe_form(tokens: &[(Token<'_>, usize, usize)], start: usize) -> usize {
    let mut pos = start + 1; // skip opening |
    while pos < tokens.len() {
        if matches!(tokens[pos].0, Token::Pipe) {
            return pos + 1;
        }
        pos = skip_one_form(tokens, pos);
    }
    pos
}

mod flatten;
pub(crate) use flatten::{
    collect_bracket_edits, collect_flatten_clause_edits, collect_flatten_edits,
};
