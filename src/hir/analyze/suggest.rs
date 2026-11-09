// audited: 2026-11-09
//! The names in scope near the spelling of a name that resolves to nothing, for its error message.
//!
//! docs/bindings.md

use super::*;

impl<'a> Analyzer<'a> {
    /// Levenshtein edit distance between two strings.
    fn levenshtein(a: &str, b: &str) -> usize {
        let m = a.len();
        let n = b.len();
        if m == 0 {
            return n;
        }
        if n == 0 {
            return m;
        }

        let mut prev: Vec<usize> = (0..=n).collect();
        let mut curr = vec![0; n + 1];

        for (i, ca) in a.chars().enumerate() {
            curr[0] = i + 1;
            for (j, cb) in b.chars().enumerate() {
                let cost = if ca == cb { 0 } else { 1 };
                curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        prev[n]
    }

    /// Find bindings in scope with names similar to `name` (edit distance <= 2).
    ///
    /// Spelling-based, so unlike resolution this does need the memo. A binding
    /// whose name this instance never learned simply cannot be suggested — the
    /// message degrades, nothing resolves differently.
    pub(super) fn suggest_similar(&self, name: &str) -> Vec<String> {
        let mut candidates: Vec<(usize, String)> = Vec::new();
        for scope in self.scopes.iter().rev() {
            for scope_sym in scope.bindings.keys() {
                let Some(scope_name) = self.symbols.name(*scope_sym) else {
                    continue;
                };
                let dist = Self::levenshtein(name, scope_name);
                if dist > 0 && dist <= 2 && !candidates.iter().any(|(_, n)| n == scope_name) {
                    candidates.push((dist, scope_name.to_string()));
                }
            }
        }
        candidates.sort_by_key(|(d, _)| *d);
        candidates.into_iter().map(|(_, n)| n).take(3).collect()
    }
}
