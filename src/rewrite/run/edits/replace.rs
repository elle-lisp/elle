// audited: 2026-10-05
//! The replace collector: a call form rewritten through a template, with the
//! rewrite applied inside each argument it places.
//!
//! docs/epochs.md

use super::*;

/// Lex source and collect edits for forms matching replace rules.
/// Works at the token level using byte offsets from the lexer.
///
/// `inner` holds the other edits the rewrite makes: a replacement builds its
/// arguments from source text, so each argument takes the edits that fall
/// inside it, and a replaced form nested in it, before it is placed. The
/// caller then drops every edit that falls inside a replacement's span.
pub(crate) fn collect_replace_edits(
    src: SourceText<'_>,
    replaces: &[(&str, usize, &str)],
    inner: &[Edit],
) -> Result<Vec<Edit>, String> {
    let tokens = src.code_tokens()?;
    replace_edits_within(src.text, &tokens, 0, tokens.len(), replaces, inner)
}

/// The replace edits for the forms in `tokens[lo..hi]`, outermost first: a
/// matched form's own nested matches go into its arguments' text.
fn replace_edits_within(
    source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    lo: usize,
    hi: usize,
    replaces: &[(&str, usize, &str)],
    inner: &[Edit],
) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();
    let mut i = lo;
    while i < hi {
        if let Some(edit) = try_match_replace(source, tokens, i, replaces, inner)? {
            i = skip_balanced_form(tokens, i);
            edits.push(edit);
        } else {
            i += 1;
        }
    }
    Ok(edits)
}

/// Try to match a replace rule at token position `i`.
/// Expects `tokens[i]` to be `LeftParen` followed by a symbol that a rule of
/// the form's arity names; a symbol may carry one rule per arity.
fn try_match_replace(
    source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    i: usize,
    replaces: &[(&str, usize, &str)],
    inner: &[Edit],
) -> Result<Option<Edit>, String> {
    // Must start with LeftParen
    if !matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
        return Ok(None);
    }
    // Next token must be a symbol that some replace rule names
    let head_sym = match tokens.get(i + 1) {
        Some((Token::Symbol(s), _, _)) => *s,
        _ => return Ok(None),
    };
    if !replaces.iter().any(|(s, _, _)| *s == head_sym) {
        return Ok(None);
    }

    // Each argument as the index of its first token and the index past its last.
    let mut args: Vec<(usize, usize)> = Vec::new();
    let mut pos = i + 2; // skip LeftParen and head symbol
    while pos < tokens.len() {
        match &tokens[pos].0 {
            Token::RightParen => break,
            _ => {
                let past = skip_one_form(tokens, pos);
                if past <= pos || past > tokens.len() {
                    return Ok(None); // malformed
                }
                args.push((pos, past));
                pos = past;
            }
        }
    }
    if pos >= tokens.len() {
        return Ok(None);
    }
    let Some(&(_, _, template)) = replaces
        .iter()
        .find(|(s, a, _)| *s == head_sym && *a == args.len())
    else {
        return Ok(None);
    };

    // Form spans from LeftParen to RightParen (inclusive)
    let form_start = tokens[i].1;
    let form_end = tokens[pos].1 + tokens[pos].2; // byte after )

    let mut texts = Vec::with_capacity(args.len());
    for &(first, past) in &args {
        texts.push(rewritten_text(
            source, tokens, first, past, replaces, inner,
        )?);
    }

    Ok(Some(Edit {
        byte_offset: form_start,
        byte_len: form_end - form_start,
        replacement: instantiate(template, &texts),
    }))
}

/// The text of `tokens[first..past]` with the rewrite applied inside it: the
/// replaced forms nested there, and every edit of `inner` that falls inside
/// the span and inside no such form.
fn rewritten_text(
    source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    first: usize,
    past: usize,
    replaces: &[(&str, usize, &str)],
    inner: &[Edit],
) -> Result<String, String> {
    let start = tokens[first].1;
    let end = tokens[past - 1].1 + tokens[past - 1].2;
    let nested = replace_edits_within(source, tokens, first, past, replaces, inner)?;
    let within =
        |e: &Edit, lo: usize, hi: usize| e.byte_offset >= lo && e.byte_offset + e.byte_len <= hi;
    let outside_nested: Vec<Edit> = inner
        .iter()
        .filter(|e| within(e, start, end))
        .filter(|e| {
            !nested
                .iter()
                .any(|n| within(e, n.byte_offset, n.byte_offset + n.byte_len))
        })
        .cloned()
        .collect();
    let mut edits: Vec<Edit> = outside_nested
        .into_iter()
        .chain(nested)
        .map(|e| Edit {
            byte_offset: e.byte_offset - start,
            ..e
        })
        .collect();
    apply_edits(&source[start..end], &mut edits)
}

/// `template` with each `$N` replaced by `args[N - 1]`, in one pass, so an
/// argument whose own text holds a `$` is placed as written.
fn instantiate(template: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let digits: String = rest[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        match digits.parse::<usize>() {
            Ok(n) if (1..=args.len()).contains(&n) => out.push_str(&args[n - 1]),
            _ => {
                out.push('$');
                out.push_str(&digits);
            }
        }
        rest = &rest[at + 1 + digits.len()..];
    }
    out.push_str(rest);
    out
}
