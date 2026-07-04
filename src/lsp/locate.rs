//! Shared cursor→symbol resolution for the position-based LSP providers.
//!
//! Hover, go-to-definition, find-references and rename all answer the same
//! first question: *which symbol is under the cursor?* They share this one
//! precise implementation, which tests exact identifier containment against
//! the name length recorded in the index.

use crate::reader::SourceLoc;
use crate::symbols::{DefId, SymbolIndex};
use serde_json::{json, Value};

/// Distance from the occurrence start if `target` lands on the identifier at
/// `loc` (which spans `name_len` columns), else `None`.
///
/// Coordinates are 1-based (`SourceLoc`'s convention). The span is treated as
/// `[col, col + name_len]` — the trailing edge is inclusive so a cursor parked
/// just past the last character (a common end-of-word position) still resolves.
fn distance_on(loc: &SourceLoc, name_len: usize, tline: usize, tcol: usize) -> Option<usize> {
    if loc.line != tline {
        return None;
    }
    if tcol < loc.col || tcol > loc.col + name_len {
        return None;
    }
    Some(tcol - loc.col)
}

fn keep_closer(best: &mut Option<(DefId, usize)>, id: DefId, dist: usize) {
    if best.is_none_or(|(_, d)| dist < d) {
        *best = Some((id, dist));
    }
}

/// Resolve the symbol under an LSP position (0-based `line`/`character`).
///
/// Considers both definition sites and usage sites of every binding; when
/// several occurrences cover the position, the one whose start is nearest wins.
pub(crate) fn symbol_at(index: &SymbolIndex, line: u32, character: u32) -> Option<DefId> {
    let tline = line as usize + 1;
    let tcol = character as usize + 1;

    let mut best: Option<(DefId, usize)> = None;
    for (id, def) in &index.definitions {
        let name_len = def.name.len();
        if let Some(loc) = index.symbol_locations.get(id) {
            if let Some(d) = distance_on(loc, name_len, tline, tcol) {
                keep_closer(&mut best, *id, d);
            }
        }
        if let Some(usages) = index.symbol_usages.get(id) {
            for loc in usages {
                if let Some(d) = distance_on(loc, name_len, tline, tcol) {
                    keep_closer(&mut best, *id, d);
                }
            }
        }
    }
    best.map(|(id, _)| id)
}

/// Bytes that may appear inside an Elle identifier token. Mirrors
/// `primitives::compile::spans::is_ident_byte`; kept local so the LSP need not
/// reach into the reflection primitives for a one-liner.
fn is_ident_byte(b: u8) -> bool {
    !b.is_ascii_whitespace()
        && !matches!(
            b,
            b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' | b'#' | b'"' | b'\''
        )
}

/// 1-based column of the standalone occurrence of `name` on `line` nearest to
/// `approx_col` (ties prefer an occurrence at or before it). `None` if absent.
fn nearest_token_col(line: &str, name: &str, approx_col: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let nb = name.as_bytes();
    let nlen = nb.len();
    if nlen == 0 || nlen > bytes.len() {
        return None;
    }
    let mut best: Option<usize> = None;
    let mut i = 0;
    while i + nlen <= bytes.len() {
        if &bytes[i..i + nlen] == nb {
            let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_ok = i + nlen >= bytes.len() || !is_ident_byte(bytes[i + nlen]);
            if before_ok && after_ok {
                let col = i + 1;
                best = Some(match best {
                    None => col,
                    Some(prev) => {
                        let (dp, dc) = (prev.abs_diff(approx_col), col.abs_diff(approx_col));
                        if dc < dp || (dc == dp && col <= approx_col) {
                            col
                        } else {
                            prev
                        }
                    }
                });
            }
        }
        i += 1;
    }
    best
}

/// Snap each definition's recorded column onto the actual name token.
///
/// The analyzer records a definition at its *initializer* span — for
/// `(def foo 1)` that is the `1`, not `foo` — while usages already point at the
/// identifier. Rename and go-to-definition must target the name, so move each
/// definition column to the nearest standalone occurrence of its name on the
/// recorded line (the name precedes the value, hence the at-or-before tiebreak).
/// Lines that do not contain the name are left as-is.
pub(crate) fn snap_definition_locations(index: &mut SymbolIndex, source: &str) {
    let lines: Vec<&str> = source.lines().collect();
    let mut fixes: Vec<(DefId, usize)> = Vec::new();
    for (id, loc) in &index.symbol_locations {
        let Some(def) = index.definitions.get(id) else {
            continue;
        };
        if loc.line == 0 || loc.line > lines.len() {
            continue;
        }
        if let Some(col) = nearest_token_col(lines[loc.line - 1], &def.name, loc.col) {
            if col != loc.col {
                fixes.push((*id, col));
            }
        }
    }
    for (id, col) in fixes {
        if let Some(loc) = index.symbol_locations.get_mut(&id) {
            loc.col = col;
        }
        if let Some(loc) = index
            .definitions
            .get_mut(&id)
            .and_then(|d| d.location.as_mut())
        {
            loc.col = col;
        }
    }
}

/// `file://` URI for a source location's originating file.
pub(crate) fn loc_uri(loc: &SourceLoc) -> String {
    format!("file://{}", loc.file)
}

/// 0-based LSP range covering an identifier of `name_len` columns at `loc`.
/// (`SourceLoc` is 1-based; LSP is 0-based.)
pub(crate) fn name_range(loc: &SourceLoc, name_len: usize) -> Value {
    let line = loc.line.saturating_sub(1);
    let start = loc.col.saturating_sub(1);
    json!({
        "start": { "line": line, "character": start },
        "end":   { "line": line, "character": start + name_len },
    })
}

/// LSP `Location` JSON (`{uri, range}`) for an identifier occurrence.
pub(crate) fn location(loc: &SourceLoc, name_len: usize) -> Value {
    json!({ "uri": loc_uri(loc), "range": name_range(loc, name_len) })
}
