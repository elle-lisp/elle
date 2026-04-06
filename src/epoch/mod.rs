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
//! (elle 42)
//! (def x 10)
//! ```
//!
//! The `(elle/epoch N)` form must be the first top-level form. It is consumed
//! by the compiler and does not appear in the expanded syntax. Files
//! without an epoch declaration are assumed to target [`CURRENT_EPOCH`].
//!
//! # Pipeline integration
//!
//! The epoch pass runs after parsing and before macro expansion:
//!
//! ```text
//! Source → Reader → [epoch migration] → Expander → HIR → LIR → Bytecode
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
    if forms.is_empty() {
        return Ok(None);
    }

    if let SyntaxKind::List(items) = &forms[0].kind {
        if items.len() == 2 && items[0].is_symbol("elle/epoch") {
            if let SyntaxKind::Int(n) = items[1].kind {
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

                // Reject duplicate epoch declarations.
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

                return Ok(Some(epoch));
            }
        }
    }

    Ok(None)
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
pub fn detect_epoch_in_source(source: &str) -> Result<Option<EpochInfo>, String> {
    // The reader strips shebang lines before parsing, so syntax spans are
    // relative to the post-strip input.  Compute the offset so we can
    // translate back to original-source byte positions.
    let shebang_offset = if source.starts_with("#!") {
        source.find('\n').map(|i| i + 1).unwrap_or(source.len())
    } else {
        0
    };

    let syntaxes = read_syntax_all(source, "<detect-epoch>")?;
    if syntaxes.is_empty() {
        return Ok(None);
    }

    if let SyntaxKind::List(items) = &syntaxes[0].kind {
        if items.len() == 2 && items[0].is_symbol("elle/epoch") {
            if let SyntaxKind::Int(n) = items[1].kind {
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
                return Ok(Some(EpochInfo {
                    epoch,
                    byte_start: syntaxes[0].span.start + shebang_offset,
                    byte_end: syntaxes[0].span.end + shebang_offset,
                }));
            }
        }
    }

    Ok(None)
}

/// Migrate forms from a source epoch to the current epoch.
///
/// Returns the number of rewrites applied. If the source epoch is
/// already current, this is a no-op.
pub fn migrate_forms(forms: &mut [Syntax], from_epoch: u64) -> Result<usize, String> {
    // Allow: CURRENT_EPOCH is 0 today so this is always-true for u64,
    // but it becomes meaningful once CURRENT_EPOCH is bumped.
    #[allow(clippy::absurd_extreme_comparisons)]
    if from_epoch >= CURRENT_EPOCH {
        return Ok(0);
    }
    transform::migrate(forms, from_epoch, CURRENT_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Span, Syntax, SyntaxKind};

    fn sym(name: &str) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(name.to_string()), Span::synthetic())
    }

    fn int(n: i64) -> Syntax {
        Syntax::new(SyntaxKind::Int(n), Span::synthetic())
    }

    fn list(items: Vec<Syntax>) -> Syntax {
        Syntax::new(SyntaxKind::List(items), Span::synthetic())
    }

    #[test]
    fn test_extract_epoch_present() {
        let mut forms = vec![
            list(vec![sym("elle/epoch"), int(0)]),
            list(vec![sym("def"), sym("x"), int(10)]),
        ];

        let epoch = extract_epoch(&mut forms).unwrap();
        assert_eq!(epoch, Some(0));
        assert_eq!(forms.len(), 1); // (elle 0) removed
    }

    #[test]
    fn test_extract_epoch_absent() {
        let mut forms = vec![list(vec![sym("def"), sym("x"), int(10)])];

        let epoch = extract_epoch(&mut forms).unwrap();
        assert_eq!(epoch, None);
        assert_eq!(forms.len(), 1); // unchanged
    }

    #[test]
    fn test_extract_epoch_empty() {
        let mut forms: Vec<Syntax> = Vec::new();
        let epoch = extract_epoch(&mut forms).unwrap();
        assert_eq!(epoch, None);
    }

    #[test]
    fn test_extract_epoch_negative() {
        let mut forms = vec![list(vec![sym("elle/epoch"), int(-1)])];
        let result = extract_epoch(&mut forms);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be non-negative"));
    }

    #[test]
    fn test_extract_epoch_future() {
        let mut forms = vec![list(vec![sym("elle/epoch"), int(CURRENT_EPOCH as i64 + 1)])];
        let result = extract_epoch(&mut forms);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("only supports up to"));
    }

    #[test]
    fn test_extract_epoch_not_elle() {
        let mut forms = vec![list(vec![sym("notelle"), int(0)])];
        let epoch = extract_epoch(&mut forms).unwrap();
        assert_eq!(epoch, None);
        assert_eq!(forms.len(), 1);
    }

    #[test]
    fn test_extract_epoch_wrong_arity() {
        let mut forms = vec![list(vec![sym("elle/epoch")])];
        let epoch = extract_epoch(&mut forms).unwrap();
        assert_eq!(epoch, None); // not recognized, left alone
    }

    #[test]
    fn test_migrate_forms_current_epoch() {
        let mut forms = vec![list(vec![sym("foo"), int(1)])];
        let count = migrate_forms(&mut forms, CURRENT_EPOCH).unwrap();
        assert_eq!(count, 0);
    }
}
