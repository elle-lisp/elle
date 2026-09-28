//! audited: 2026-09-28
//! Keyword identity and the static keyword vocabulary.
//!
//! A keyword's payload is the 64-bit FNV-1a hash of its name — the same
//! function symbol identity uses ([`crate::namehash`]), so equality is
//! `u64 == u64` with no string comparison and no heap dereference, and the
//! same name yields the same payload in every thread, process, and build.
//!
//! Construction is identity only: [`Value::keyword`](crate::value::Value)
//! records nothing. Display resolves a spelling through the per-instance
//! memo first (`SymbolTable::keyword_name`), then through `VOCABULARY` —
//! the build-fixed list of every spelling the Rust runtime itself mints.
//! The vocabulary is the keyword analogue of the primitive-name index in
//! `primitives::registration::static_name`: fixed at build time, immutable,
//! and needing no instance. A spelling in neither renders as
//! `#<keyword:hash>` (docs/impl/symbol.md § "Reading a name, and not
//! reading one").

use crate::symbol::SymbolTable;
use std::collections::HashMap;
use std::sync::LazyLock;

mod vocabulary;
pub(crate) use vocabulary::VOCABULARY;

/// The 64-bit name hash of a keyword name — [`crate::namehash::name_hash`],
/// the same function symbol identity uses.
pub const fn keyword_hash(name: &str) -> u64 {
    crate::namehash::name_hash(name)
}

/// Whether `VOCABULARY` carries `name`. `const fn`, so a caller can ask in a
/// `const` context and turn the answer into a build error; the linear scan is
/// what makes it one (the `static_keyword_name` index is not const-buildable).
pub const fn is_vocabulary(name: &str) -> bool {
    let hash = keyword_hash(name);
    let mut i = 0;
    while i < VOCABULARY.len() {
        if keyword_hash(VOCABULARY[i]) == hash {
            return true;
        }
        i += 1;
    }
    false
}

/// `name`, having asserted the vocabulary carries it.
///
/// The spelling of a keyword the runtime coins from a fixed string has to be
/// in `VOCABULARY` or the keyword has no name to print, and `json/serialize`
/// refuses any struct that carries it as a key. Called from a `const` block,
/// this moves that requirement to compile time: `const { vocab("input") }` is
/// a build error at the line that wrote it until "input" is listed.
///
/// `rich_error!` is the caller that needs it. A field name written
/// `input = …` reaches the keyword constructor through `stringify!`, as a
/// token rather than as a string, so no scan of the source can find it
/// (docs/impl/symbol.md § "A spelling the runtime itself mints").
pub const fn vocab(name: &'static str) -> &'static str {
    assert!(
        is_vocabulary(name),
        "keyword spelling missing from VOCABULARY in src/value/keyword.rs"
    );
    name
}

/// The spelling of a vocabulary keyword, or `None` if `hash` names nothing
/// the runtime spells itself.
pub(crate) fn static_keyword_name(hash: u64) -> Option<&'static str> {
    static INDEX: LazyLock<HashMap<u64, &'static str>> = LazyLock::new(|| {
        let mut index = HashMap::new();
        for name in VOCABULARY {
            index.insert(keyword_hash(name), *name);
        }
        index
    });
    INDEX.get(&hash).copied()
}

/// Resolve a keyword payload to its spelling: the instance memo first, then
/// the static vocabulary. The one lookup order every display and
/// name-recovering site uses.
pub(crate) fn resolve_keyword_name(memo: Option<&SymbolTable>, hash: u64) -> Option<&str> {
    memo.and_then(|m| m.keyword_name(hash))
        .or_else(|| static_keyword_name(hash))
}

#[cfg(test)]
mod tests;
