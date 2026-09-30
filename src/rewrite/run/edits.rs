// audited: 2026-09-30
// The edit collectors behind `elle rewrite`: one per migration rule kind, each
// turning a rule into byte-span edits over the source text.
// docs/epochs.md

use super::*;
use crate::epoch::rules::TimeArg;
use crate::epoch::seconds::{wrap_ends, Millis, Seconds, Written};

/// Scan source for removed symbols and return an error listing them.
pub(super) fn check_removals(
    src: SourceText<'_>,
    removals: &HashMap<&str, &str>,
) -> Result<(), String> {
    let tokens = src.code_tokens()?;

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
            src.name,
            errors.join("\n")
        ))
    }
}

/// Lex source and collect edits for forms matching unwrap rules.
/// Matches `(symbol (fn [] body...))` or `(symbol (fn () body...))` and
/// replaces the entire form with just the body.
pub(super) fn collect_unwrap_edits(
    src: SourceText<'_>,
    unwraps: &HashMap<&str, &str>,
) -> Result<Vec<Edit>, String> {
    let tokens = src.code_tokens()?;

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(edit) = try_match_unwrap(src.text, &tokens, i, unwraps) {
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
                            src.name, name, msg
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
pub(crate) fn collect_replace_edits(
    src: SourceText<'_>,
    replaces: &[(&str, usize, &str)],
) -> Result<Vec<Edit>, String> {
    let tokens = src.code_tokens()?;

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(edit) = try_match_replace(src.text, &tokens, i, replaces) {
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

/// Collect edits that spell every named reader shorthand out as its form:
/// `;x` becomes `(splice x)` (docs/impl/lexicon.md).
///
/// Each shorthand yields two edits rather than one span over the whole
/// `<prefix><form>` text. A single span would have to build its replacement
/// from source bytes, and a shorthand nested in those bytes would ride along
/// unrewritten; two edits let the inner one rewrite itself.
pub(super) fn collect_desugar_edits(
    src: SourceText<'_>,
    shorthands: &[Token<'static>],
) -> Result<Vec<Edit>, String> {
    if shorthands.is_empty() {
        return Ok(Vec::new());
    }
    let tokens = src.code_tokens()?;
    let shebang = crate::reader::shebang_len(src.text);

    let mut edits = Vec::new();
    for i in 0..tokens.len() {
        let (token, offset, _) = &tokens[i];
        if !shorthands.contains(token) {
            continue;
        }
        // The shebang line is the operating system's, however this lexicon
        // tokenized it (`SourceText::in_shebang`).
        if *offset < shebang {
            continue;
        }
        let Some(form) = token.shorthand_form() else {
            return Err(format!(
                "{}: {:?} names no form to desugar into",
                src.name, token
            ));
        };

        // A prefix with no form after it — `(f ;)` or a `;` at end of input —
        // does not parse. The reader reports that. Wrapping whatever token
        // comes next would rewrite it into different broken text, and a
        // closing delimiter is the case that looks like a form and is not.
        let Some((next, form_start, _)) = tokens.get(i + 1) else {
            continue;
        };
        if matches!(
            next,
            Token::RightParen | Token::RightBracket | Token::RightBrace
        ) {
            continue;
        }

        // The extent of the form this prefix wraps. `skip_one_form` walks the
        // prefix and then that form, so it answers the index just past it.
        let end = skip_one_form(&tokens, i);
        if end <= i + 1 || end > tokens.len() {
            continue;
        }
        let (_, last_start, last_len) = tokens[end - 1];
        let form_end = last_start + last_len;

        // From the prefix to the start of the form, so any whitespace the
        // author left between them is absorbed by the one space in `(form `.
        edits.push(Edit {
            byte_offset: *offset,
            byte_len: form_start - offset,
            replacement: format!("({} ", form),
        });
        edits.push(Edit {
            byte_offset: form_end,
            byte_len: 0,
            replacement: ")".to_string(),
        });
    }
    Ok(edits)
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

/// Collect the edits that rewrite each millisecond duration a call named in
/// `millis` gave as a `:timeout` in seconds (docs/epochs.md).
///
/// The edits are token-level, like the renames: a literal is replaced where
/// it stands, an argument that meant no bound is deleted with the space before
/// it, and an expression is wrapped by one insertion at each end. The two
/// insertions leave the expression's own bytes to the other edits, so a rename
/// inside it still lands. A quoted form is data and is left as written.
pub(super) fn collect_millis_edits(
    src: SourceText<'_>,
    millis: &HashMap<&str, TimeArg>,
) -> Result<Vec<Edit>, String> {
    if millis.is_empty() {
        return Ok(Vec::new());
    }
    let tokens = src.code_tokens()?;
    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i].0 {
            Token::Quote if i + 1 < tokens.len() => {
                i = skip_one_form(&tokens, i);
                continue;
            }
            Token::LeftParen => {
                if let Some((Token::Symbol(head), _, _)) = tokens.get(i + 1) {
                    if let Some(place) = millis.get(head) {
                        edits.extend(millis_call_edits(&tokens, i, *place));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(edits)
}

/// The edits for one call whose `(` is `tokens[open]`. None when the call does
/// not have the shape `place` names, or does not close.
fn millis_call_edits(
    tokens: &[(Token<'_>, usize, usize)],
    open: usize,
    place: TimeArg,
) -> Vec<Edit> {
    // Each argument as the index of its first token and the index past its last.
    let mut args: Vec<(usize, usize)> = Vec::new();
    let mut pos = open + 2;
    while pos < tokens.len() && !matches!(tokens[pos].0, Token::RightParen) {
        let end = skip_one_form(tokens, pos);
        if end > tokens.len() {
            return Vec::new();
        }
        args.push((pos, end));
        pos = end;
    }
    if pos >= tokens.len() {
        return Vec::new();
    }
    let start_of = |(first, _): (usize, usize)| tokens[first].1;
    let end_of = |(_, past): (usize, usize)| tokens[past - 1].1 + tokens[past - 1].2;
    let shape = |(first, past): (usize, usize)| -> Millis {
        if past != first + 1 {
            return Millis::Expr;
        }
        match tokens[first].0 {
            Token::Integer(n) => Millis::Int(n),
            Token::Float(x) => Millis::Float(x),
            Token::Nil => Millis::Nil,
            _ => Millis::Expr,
        }
    };
    // The edits that write `value` anew, with `keyword` before it.
    let write = |value: (usize, usize), written: Written, keyword: &str| match written {
        Written::Literal(seconds) => vec![Edit {
            byte_offset: start_of(value),
            byte_len: end_of(value) - start_of(value),
            replacement: format!("{keyword}{seconds}"),
        }],
        Written::Wrap(template) => {
            let (before, after) = wrap_ends(template);
            vec![
                Edit {
                    byte_offset: start_of(value),
                    byte_len: 0,
                    replacement: format!("{keyword}{before}"),
                },
                Edit {
                    byte_offset: end_of(value),
                    byte_len: 0,
                    replacement: after.to_string(),
                },
            ]
        }
    };
    match place {
        TimeArg::Keyword => {
            let Some(at) = args.iter().position(
                |&(first, past)| matches!(tokens[first].0, Token::Keyword("timeout") if past == first + 1),
            ) else {
                return Vec::new();
            };
            let Some(&value) = args.get(at + 1) else {
                return Vec::new();
            };
            match Seconds::of(shape(value), place) {
                Seconds::Write(written) => write(value, written, ""),
                Seconds::Keep | Seconds::Drop => Vec::new(),
            }
        }
        TimeArg::Last { arity, .. } => {
            if args.len() != arity {
                return Vec::new();
            }
            let value = args[arity - 1];
            match Seconds::of(shape(value), place) {
                Seconds::Keep => Vec::new(),
                Seconds::Drop => {
                    // From the end of what precedes the argument — the head
                    // symbol for a call of one argument — to its own end.
                    let before = tokens[value.0 - 1].1 + tokens[value.0 - 1].2;
                    vec![Edit {
                        byte_offset: before,
                        byte_len: end_of(value) - before,
                        replacement: String::new(),
                    }]
                }
                Seconds::Write(written) => write(value, written, ":timeout "),
            }
        }
    }
}
