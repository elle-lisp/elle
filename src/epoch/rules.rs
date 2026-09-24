// audited: 2026-09-23
//! Epoch migration rule definitions.
//!
//! docs/epochs.md
//!
//! Each breaking change to Elle increments the epoch counter and adds
//! migration rules here. Rules are pure data so they can be consumed
//! by both the in-pipeline transformer and the `elle rewrite` CLI tool.

mod migrations;

use crate::reader::escape::StringEscapes;
use crate::reader::Token;
use migrations::MIGRATIONS;
use std::collections::HashMap;

/// Current language epoch. Bump this when making a breaking change
/// and add a corresponding entry to `MIGRATIONS`.
pub const CURRENT_EPOCH: u64 = 13;

/// The epoch-gated lexer rules (docs/impl/lexicon.md). The lexer consults
/// a `Lexicon` instead of hard-coding these, so an epoch bump can change
/// tokenization itself. Epochs 0 to 12 share one lexicon; epoch 13 reads
/// string escapes by stricter rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lexicon {
    /// The character that starts a comment running to end of line.
    pub(crate) comment_char: char,
    /// Whether `;` lexes as the splice token. The comment character is
    /// checked first, so a lexicon whose `comment_char` is `;` never
    /// consults this flag; with both off, `;` is a lex error.
    pub(crate) semicolon_splices: bool,
    /// Whether `,;` fuses into the unquote-splicing token.
    pub(crate) comma_semicolon_fuses: bool,
    /// Which escapes a string literal accepts.
    pub(crate) string_escapes: StringEscapes,
}

impl Lexicon {
    /// The lexicon for the given epoch. Callers validate the epoch number
    /// first: the prescan and `extract_epoch` both reject epochs above
    /// [`CURRENT_EPOCH`].
    pub fn for_epoch(epoch: u64) -> Lexicon {
        debug_assert!(epoch <= CURRENT_EPOCH, "unregistered epoch {epoch}");
        Lexicon {
            comment_char: '#',
            semicolon_splices: true,
            comma_semicolon_fuses: true,
            string_escapes: if epoch < 13 {
                StringEscapes::Lenient
            } else {
                StringEscapes::Strict
            },
        }
    }

    /// The hint a lex error appends when an older epoch reads the text this
    /// lexicon refused: the declaration that restores that reading
    /// (docs/impl/lexicon.md). `reads` answers whether a lexicon reads the
    /// text. `None` when no older epoch reads it either.
    pub(crate) fn older_reading_hint(&self, reads: impl Fn(&Lexicon) -> bool) -> Option<String> {
        let epoch = (0..=CURRENT_EPOCH).rev().find(|&epoch| {
            let older = Lexicon::for_epoch(epoch);
            older != *self && reads(&older)
        })?;
        Some(format!(
            "; if this file targets epoch {epoch} or earlier, declare \
             (elle/epoch {epoch}) as its first form, then run `elle rewrite`"
        ))
    }

    /// The current epoch's lexicon.
    pub fn current() -> Lexicon {
        Lexicon::for_epoch(CURRENT_EPOCH)
    }

    /// The text that spells `token` — read under `self` from the source text
    /// `lexeme` — with its meaning intact under `target`. `None` when both
    /// lexicons spell it alike, which is every token whose rules did not move.
    ///
    /// This is the only place a token crosses between two lexicons, so
    /// `elle rewrite` and the lexer read the same fields and cannot drift
    /// apart (docs/impl/lexicon.md).
    ///
    /// A token `target` cannot spell at all is an error, not an unchanged
    /// token: leaving those bytes in place would keep a file that parses
    /// and means something else.
    pub(crate) fn respell(
        &self,
        token: &Token<'_>,
        lexeme: &str,
        target: &Lexicon,
    ) -> Result<Option<String>, String> {
        match token {
            // A comment is its introducer and the rest of the line. Only the
            // introducer belongs to the lexicon; the rest is the author's.
            Token::Comment(text) if self.comment_char != target.comment_char => {
                let body = text.strip_prefix(self.comment_char).unwrap_or(text);
                Ok(Some(format!("{}{}", target.comment_char, body)))
            }
            // The token holds the decoded string, so the escapes are read
            // from the source text.
            Token::String(_) if self.string_escapes != target.string_escapes => {
                self.string_escapes.respell(lexeme, target.string_escapes)
            }
            Token::Splice if self.semicolon_splices && !target.semicolon_splices => {
                Err(no_spelling("`;` splice"))
            }
            Token::UnquoteSplicing
                if self.comma_semicolon_fuses && !target.comma_semicolon_fuses =>
            {
                Err(no_spelling("`,;` unquote-splicing"))
            }
            _ => Ok(None),
        }
    }
}

/// The refusal shared by every token shape that survives a lexical change
/// only as a different form, never as different bytes.
fn no_spelling(lexeme: &str) -> String {
    format!(
        "{} has no spelling under the target lexicon; the epoch that removed \
         the shape migrates it with a tree rule, not a token rewrite",
        lexeme
    )
}

/// Lexicons that match no registered epoch. Tests lex under them to prove
/// the lexer consults its lexicon rather than hard-coding the rules.
#[cfg(test)]
impl Lexicon {
    /// `;` comments, nothing splices — the shape issue #983 proposes.
    pub(crate) fn divergent() -> Lexicon {
        Lexicon {
            comment_char: ';',
            semicolon_splices: false,
            comma_semicolon_fuses: false,
            ..Lexicon::current()
        }
    }

    /// `;` has no meaning at all: not a comment, not a splice.
    pub(crate) fn no_semicolon() -> Lexicon {
        Lexicon {
            semicolon_splices: false,
            comma_semicolon_fuses: false,
            ..Lexicon::current()
        }
    }
}

/// A token-level change an epoch made, for the reader of a changelog.
///
/// The rewrite itself comes from `Lexicon::respell`; this only names the
/// change so `elle rewrite --list-rules` and the epoch history can show it
/// beside the tree rules (docs/impl/lexicon.md).
#[derive(Debug, Clone)]
pub struct LexicalChange {
    /// Short name, in the shape of a rule name: `comment-char`, `splice`.
    pub name: &'static str,
    /// Human-readable summary for changelogs and error messages.
    pub summary: &'static str,
}

/// A set of changes introduced at a given epoch.
#[derive(Debug, Clone)]
pub struct Migration {
    /// The epoch these rules migrate TO (from epoch - 1).
    pub epoch: u64,
    /// Human-readable summary for changelogs and error messages.
    pub summary: &'static str,
    /// The individual rules in this migration.
    pub rules: &'static [MigrationRule],
    /// The token-level changes in this migration. Non-empty exactly when
    /// this epoch's [`Lexicon`] differs from its predecessor's; a test
    /// pins the pairing.
    pub lexical: &'static [LexicalChange],
}

/// A single mechanical transformation.
#[derive(Debug, Clone)]
pub enum MigrationRule {
    /// Rename a symbol: all occurrences of `old` become `new`.
    Rename {
        old: &'static str,
        new: &'static str,
    },
    /// A form has been removed. Any occurrence of this symbol in head
    /// position of a list emits the provided error message.
    Remove {
        symbol: &'static str,
        message: &'static str,
    },
    /// Unwrap a call that wraps a single zero-arg lambda. Matches
    /// `(symbol (fn [] body...))` or `(symbol (fn () body...))` and
    /// replaces with `(begin body...)`. If the form doesn't match this
    /// pattern, produces a compile error with `message` (like Remove).
    Unwrap {
        symbol: &'static str,
        message: &'static str,
    },
    /// Replace a call form structurally. Matches `(symbol arg1 ... argN)`
    /// by head symbol and arity, then rewrites using a template with
    /// positional placeholders `$1`, `$2`, etc.
    Replace {
        symbol: &'static str,
        arity: usize,
        template: &'static str,
    },
    /// Flatten nested-pair binding vectors into flat alternating pairs.
    /// Matches `(symbol [[p1 v1] [p2 v2] ...] body...)` where the
    /// bindings container has children that are all 2-element lists/arrays,
    /// and splices each child's contents into the parent container.
    FlattenBindings { symbols: &'static [&'static str] },
    /// Flatten parenthesized clauses into flat pairs.
    /// Matches `(symbol <skip args> (test body) (test body) ...)` and
    /// splices each clause's contents flat. Multi-body arms get `(begin ...)`.
    /// `(else body)` in cond becomes just the body as a trailing default.
    FlattenClauses {
        symbols: &'static [&'static str],
        /// Number of leading args to skip (0 for cond, 1 for match — the value expr)
        skip: usize,
    },
    /// Replace a reader shorthand with the form it stands for: `;x` becomes
    /// `(splice x)`. An epoch declares this when it takes the shorthand's
    /// spelling away, so the rule always accompanies a lexical change.
    ///
    /// The rule names only the token. Which form the shorthand stands for is
    /// the reader's to say, and [`Token::shorthand_form`] says it, so an
    /// epoch can neither desugar a token that wraps nothing nor name the
    /// wrong form for one that does.
    ///
    /// Only `elle rewrite` consumes it. The shorthand and the form read to
    /// the same tree, so an old file compiles without any tree rewrite —
    /// which is also the condition an epoch must check before declaring one
    /// (docs/impl/lexicon.md § "Desugaring a reader shorthand").
    Desugar { shorthand: Token<'static> },
}

/// Get all migrations for epochs in the range (from, to].
pub fn migrations_in_range(from: u64, to: u64) -> impl Iterator<Item = &'static Migration> {
    MIGRATIONS
        .iter()
        .filter(move |m| m.epoch > from && m.epoch <= to)
}

/// Every token-level change made in the range (from, to].
pub fn lexical_changes_in_range(
    from: u64,
    to: u64,
) -> impl Iterator<Item = &'static LexicalChange> {
    migrations_in_range(from, to).flat_map(|m| m.lexical.iter())
}

/// Collapse all renames in a range into a single lookup table.
///
/// Chains renames across epochs: if epoch 1 renames A→B and epoch 2
/// renames B→C, the collapsed table maps A→C directly.
pub fn collapsed_renames(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut table: HashMap<&'static str, &'static str> = HashMap::new();

    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Rename { old, new } = rule {
                // If something already maps to `old`, chase the chain.
                let original = table.iter().find(|(_, v)| *v == old).map(|(k, _)| *k);

                if let Some(original) = original {
                    table.insert(original, new);
                } else {
                    table.insert(old, new);
                }
            }
        }
    }

    table
}

/// Collect all replace rules in a range as (symbol, arity, template) tuples.
pub fn replace_rules_in_range(from: u64, to: u64) -> Vec<(&'static str, usize, &'static str)> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Replace {
                symbol,
                arity,
                template,
            } = rule
            {
                result.push((*symbol, *arity, *template));
            }
        }
    }
    result
}

/// Collect all unwrap rules in a range as (symbol, message) pairs.
pub fn unwrap_rules_in_range(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut result = HashMap::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Unwrap { symbol, message } = rule {
                result.insert(*symbol, *message);
            }
        }
    }
    result
}

/// Collect all flatten-bindings rules in a range as sets of symbols.
pub fn flatten_rules_in_range(from: u64, to: u64) -> Vec<&'static str> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::FlattenBindings { symbols } = rule {
                for sym in *symbols {
                    if !result.contains(sym) {
                        result.push(sym);
                    }
                }
            }
        }
    }
    result
}

/// Collect all flatten-clauses rules in a range as (symbol, skip) pairs.
pub fn flatten_clause_rules_in_range(from: u64, to: u64) -> Vec<(&'static str, usize)> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::FlattenClauses { symbols, skip } = rule {
                for sym in *symbols {
                    if !result.iter().any(|(s, _)| s == sym) {
                        result.push((*sym, *skip));
                    }
                }
            }
        }
    }
    result
}

/// Collect the shorthand tokens every desugar rule in a range names.
pub fn desugar_rules_in_range(from: u64, to: u64) -> Vec<Token<'static>> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Desugar { shorthand } = rule {
                result.push(shorthand.clone());
            }
        }
    }
    result
}

/// Collect all removals in a range as (symbol, message) pairs.
pub fn removals_in_range(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut result = HashMap::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Remove { symbol, message } = rule {
                result.insert(*symbol, *message);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests;
