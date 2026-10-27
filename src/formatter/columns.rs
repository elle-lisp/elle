// audited: 2026-10-25
//! The column check of `elle fmt --check`: where each code delimiter of the
//! formatted text sits.
//!
//! docs/fmt.md

use crate::formatter::comments::{lex_for_format, strip_shebang};
use crate::reader::{lexicon_for, OwnedToken};

/// The 0-based line and column of each CODE opening delimiter in `text`.
///
/// The reader's lexer finds them, so a `(`, `[`, or `{` inside a string
/// literal, on any line the string spans, or inside a comment is text: the
/// formatter places those characters itself, and `--check` must not reject
/// `elle fmt`'s own output. The column is that of the bracket, the last
/// character of a `b[` or `@b[` token.
fn code_delimiters(text: &str) -> Result<Vec<(usize, usize)>, String> {
    let (stripped, shebang) = strip_shebang(text);
    let lexed = lex_for_format(stripped, "<check>", lexicon_for(stripped)?)?;
    let shebang_lines = shebang.lines().count();
    let opens = lexed
        .tokens
        .iter()
        .zip(&lexed.locations)
        .zip(&lexed.lengths);
    Ok(opens
        .filter(|((token, _), _)| {
            matches!(
                token,
                OwnedToken::LeftParen
                    | OwnedToken::LeftBracket
                    | OwnedToken::LeftBrace
                    | OwnedToken::BytesBracket
                    | OwnedToken::AtBytesBracket
            )
        })
        .map(|((_, loc), len)| (shebang_lines + loc.line - 1, loc.col + len - 2))
        .collect())
}

/// Check for lines with opening delimiters past column thresholds.
/// Returns Some(exit_code) if violations found, None if clean.
pub(super) fn check_columns(file_path: &str, formatted: &str, line_length: usize) -> Option<i32> {
    let mut has_warning = false;
    let mut has_error = false;
    let warn_col = line_length * 3 / 4; // 75% of line length (60 for default 80)

    let delimiters = match code_delimiters(formatted) {
        Ok(delimiters) => delimiters,
        Err(e) => {
            eprintln!(
                "error: {}: cannot read the formatted text: {}",
                file_path, e
            );
            return Some(1);
        }
    };
    for (line_num, col) in delimiters {
        if col >= line_length {
            eprintln!(
                "error: {}:{}:{}: opening delimiter past column {}",
                file_path,
                line_num + 1,
                col + 1,
                line_length,
            );
            has_error = true;
        } else if col >= warn_col {
            eprintln!(
                "warning: {}:{}:{}: opening delimiter past column {}, consider refactoring",
                file_path,
                line_num + 1,
                col + 1,
                warn_col,
            );
            has_warning = true;
        }
    }

    if has_error {
        Some(1)
    } else if has_warning {
        Some(0)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
