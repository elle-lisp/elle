//! Epoch-based migration system.
//!
//! Each breaking change to Elle increments the epoch counter and adds
//! migration rules. Source files can declare their epoch with `(elle/epoch N)`
//! as the first form. The compiler transparently rewrites old-epoch
//! syntax before macro expansion.
//!
//! # File format
//!
//! ```lisp
//! (elle/epoch 12)
//! (def x 10)
//! ```
//!
//! The `(elle/epoch N)` form must be the first top-level form. It is consumed
//! by the compiler and does not appear in the expanded syntax. Files
//! without an epoch declaration are assumed to target [`CURRENT_EPOCH`].
//!
//! # Pipeline integration
//!
//! The reader prescans the declaration to choose the lexicon it tokenizes
//! under (docs/impl/lexicon.md). The migration pass then runs after parsing
//! and before macro expansion:
//!
//! ```text
//! Source → [epoch prescan] → Reader → [epoch migration] → Expander → HIR
//!        → LIR → Bytecode
//! ```

pub mod rules;
pub mod transform;

pub use rules::CURRENT_EPOCH;

use crate::reader::read_syntax_all;
use crate::syntax::{Syntax, SyntaxKind};

/// Extract the epoch declaration from parsed forms, if present.
///
/// Looks for `(elle/epoch N)` as the first form. If found, removes it from
/// the list and returns the epoch number. If absent, returns `None`
/// (the file targets the current epoch).
pub fn extract_epoch(forms: &mut Vec<Syntax>) -> Result<Option<u64>, String> {
    let Some(n) = forms.first().and_then(epoch_declaration) else {
        return Ok(None);
    };
    if n < 0 {
        return Err(format!(
            "invalid epoch at {}: {} (must be non-negative)",
            forms[0].span, n
        ));
    }
    let epoch = n as u64;
    if epoch > CURRENT_EPOCH {
        return Err(format!(
            "file at {} targets epoch {} but this compiler only supports up to epoch {}",
            forms[0].span, epoch, CURRENT_EPOCH
        ));
    }
    forms.remove(0);

    // Reject duplicate epoch declarations. This matches the form's shape
    // rather than a usable number, which `epoch_declaration` requires:
    // `(elle/epoch x)` further down the file is still a second declaration.
    for form in forms.iter() {
        if let SyntaxKind::List(items) = &form.kind {
            if items.len() == 2 && items[0].is_symbol("elle/epoch") {
                return Err(format!(
                    "duplicate (elle/epoch) at {}; only one epoch declaration is allowed per file",
                    form.span
                ));
            }
        }
    }

    Ok(Some(epoch))
}

/// The number in an `(elle/epoch N)` form, or `None` when `form` is not
/// one. The number is unvalidated: callers decide what they can act on.
fn epoch_declaration(form: &Syntax) -> Option<i64> {
    let SyntaxKind::List(items) = &form.kind else {
        return None;
    };
    if items.len() != 2 || !items[0].is_symbol("elle/epoch") {
        return None;
    }
    match items[1].kind {
        SyntaxKind::Int(n) => Some(n),
        _ => None,
    }
}

/// The epoch whose lexicon tokenizes this source (docs/impl/lexicon.md).
///
/// Scans a frozen micro-grammar that no epoch may change: an optional
/// shebang line, whitespace, then the literal shape `(elle/epoch N)`.
/// Anything else means the source targets [`CURRENT_EPOCH`].
///
/// Comment syntax is epoch-dependent, so the prescan cannot skip a
/// comment without the answer it is computing. A declaration below a
/// comment is therefore invisible here — `extract_epoch` still sees it
/// for tree migration, and the reader rejects the file only when the two
/// epochs select different lexicons.
pub fn prescan_epoch(source: &str) -> Result<u64, String> {
    let rest = &source[crate::reader::shebang_len(source)..];
    let Some(rest) = rest.trim_start().strip_prefix('(') else {
        return Ok(CURRENT_EPOCH);
    };
    let Some(rest) = rest.trim_start().strip_prefix("elle/epoch") else {
        return Ok(CURRENT_EPOCH);
    };
    // The symbol must end here: `elle/epochs` is a different name.
    if !rest.starts_with(|c: char| c.is_whitespace()) {
        return Ok(CURRENT_EPOCH);
    }
    let rest = rest.trim_start();
    let digits = &rest[..rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len())];
    if digits.is_empty() || !rest[digits.len()..].trim_start().starts_with(')') {
        return Ok(CURRENT_EPOCH);
    }
    match digits.parse::<u64>() {
        Ok(epoch) if epoch <= CURRENT_EPOCH => Ok(epoch),
        // Also the overflow case: an epoch too large for u64 is too new.
        _ => Err(format!(
            "file targets epoch {} but this compiler only supports up to epoch {}",
            digits, CURRENT_EPOCH
        )),
    }
}

/// An epoch paired with the lexicon it selects.
///
/// The mismatch check compares two of these. Carrying the lexicon beside
/// its epoch lets a test build a pair no registered epoch can produce:
/// every registered epoch shares one lexicon today, so the refusal below
/// is otherwise unreachable and would ship untested.
#[derive(Clone, Copy)]
struct EpochLexicon {
    epoch: u64,
    lexicon: rules::Lexicon,
}

impl EpochLexicon {
    /// The pair a real epoch number produces.
    fn of(epoch: u64) -> Self {
        EpochLexicon {
            epoch,
            lexicon: rules::Lexicon::for_epoch(epoch),
        }
    }

    /// A pair no registered epoch produces, for reaching the refusal path.
    #[cfg(test)]
    fn with_lexicon(epoch: u64, lexicon: rules::Lexicon) -> Self {
        EpochLexicon { epoch, lexicon }
    }
}

/// Reject a source whose declaration could not have chosen the lexer that
/// read it (docs/impl/lexicon.md).
fn refuse_mismatch(
    declared: EpochLexicon,
    prescanned: EpochLexicon,
    source_name: &str,
) -> Result<(), String> {
    // Lexicons, never epoch numbers. A declaration the prescan could not
    // see still tokenized correctly when both epochs lex alike, which is
    // every file today and every file that carries a comment above its
    // declaration.
    if declared.lexicon == prescanned.lexicon {
        return Ok(());
    }
    Err(format!(
        "{}: this file declares (elle/epoch {}), but the reader tokenized it \
         as epoch {}. The two epochs do not lex alike. Move the declaration \
         above everything except a shebang line.",
        source_name, declared.epoch, prescanned.epoch
    ))
}

/// The epoch this tree declares, when the declaration is one the lexicon
/// tables can answer for.
///
/// [`extract_epoch`] is the authority: it validates the number, consumes
/// the form, and reports a negative or unknown epoch. This only looks, so
/// the lexicon check can run before it.
fn declared_epoch(forms: &[Syntax]) -> Option<u64> {
    let n = epoch_declaration(forms.first()?)?;
    u64::try_from(n).ok().filter(|e| *e <= CURRENT_EPOCH)
}

/// Reject a file whose epoch declaration the prescan could not see, when
/// the declaration and the prescan select different lexicons
/// (docs/impl/lexicon.md). Pass the source as the reader received it.
pub fn check_lexicon_agreement(
    forms: &[Syntax],
    source: &str,
    source_name: &str,
) -> Result<(), String> {
    match declared_epoch(forms) {
        Some(declared) => check_declared_lexicon(declared, source, source_name),
        None => Ok(()),
    }
}

/// The same check for a caller that already knows the declared epoch and
/// has no parsed tree — `elle rewrite`, which reads the declaration out of
/// the source text.
pub fn check_declared_lexicon(
    declared: u64,
    source: &str,
    source_name: &str,
) -> Result<(), String> {
    let prescanned = crate::reader::prescanned_epoch_for(source, source_name)?;
    refuse_mismatch(
        EpochLexicon::of(declared),
        EpochLexicon::of(prescanned),
        source_name,
    )
}

/// Info about an epoch declaration found in source text.
pub struct EpochInfo {
    /// The declared epoch number.
    pub epoch: u64,
    /// Byte offset of `(elle/epoch N)` in the source (start).
    pub byte_start: usize,
    /// Byte offset of `(elle/epoch N)` in the source (end, exclusive).
    pub byte_end: usize,
}

/// Detect the epoch declaration from raw source text.
///
/// Parses just enough to find `(elle/epoch N)` at the start. Returns `None`
/// if no epoch declaration is present. Used by the CLI rewriter to
/// build per-file rules without modifying the syntax tree.
///
/// The read goes through [`read_syntax_all`], so the source is tokenized
/// under the lexicon its own prescan selects (docs/impl/lexicon.md) — the
/// spans below name bytes of the file as its author wrote it.
pub fn detect_epoch_in_source(source: &str) -> Result<Option<EpochInfo>, String> {
    // The reader strips shebang lines before parsing, so syntax spans are
    // relative to the post-strip input.  Compute the offset so we can
    // translate back to original-source byte positions.
    let shebang_offset = crate::reader::shebang_len(source);

    // Standalone: this runs from the CLI rewriter, with no runtime in reach.
    let mut home = crate::syntax::SyntaxHeap::new();
    let syntaxes = read_syntax_all(home.arena(), source, "<detect-epoch>")?;
    let Some(n) = syntaxes.first().and_then(epoch_declaration) else {
        return Ok(None);
    };
    if n < 0 {
        return Err(format!("invalid epoch: {} (must be non-negative)", n));
    }
    let epoch = n as u64;
    if epoch > CURRENT_EPOCH {
        return Err(format!(
            "file targets epoch {} but this compiler only supports up to epoch {}",
            epoch, CURRENT_EPOCH
        ));
    }
    Ok(Some(EpochInfo {
        epoch,
        byte_start: syntaxes[0].span.start as usize + shebang_offset,
        byte_end: syntaxes[0].span.end as usize + shebang_offset,
    }))
}

/// Migrate forms from a source epoch to the current epoch.
///
/// Returns the number of rewrites applied. If the source epoch is
/// already current, this is a no-op.
pub fn migrate_forms(
    arena: &crate::syntax::SyntaxArena,
    forms: &mut [Syntax],
    from_epoch: u64,
) -> Result<usize, String> {
    // Allow: CURRENT_EPOCH is 0 today so this is always-true for u64,
    // but it becomes meaningful once CURRENT_EPOCH is bumped.
    #[allow(clippy::absurd_extreme_comparisons)]
    if from_epoch >= CURRENT_EPOCH {
        return Ok(0);
    }
    transform::migrate(arena, forms, from_epoch, CURRENT_EPOCH)
}

#[cfg(test)]
mod tests;
