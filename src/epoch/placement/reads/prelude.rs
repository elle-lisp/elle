// audited: 2026-11-20
//! The prelude's binding macros as the read scan reads them: a name the macro binds hides a read inside it.
//!
//! docs/epochs/bodies.md
//!
//! Migration runs before expansion, so the scan reads each macro by the shape
//! its prelude definition expands to.

use super::{bound, hidden, without, Scan};
use crate::syntax::Syntax;

impl Scan<'_> {
    /// Whether `items`, a call of one of the prelude's binding macros, reads
    /// one of `names`, or `None` when it calls no such macro. The else of an
    /// `if-let` stands inside the binding of its first name alone.
    pub(super) fn binding_macro(&self, items: &[Syntax], names: &[&str]) -> Option<bool> {
        let head = items.first().and_then(Syntax::as_symbol)?;
        let read = match head {
            "if-let" | "when-let" if items.len() >= 2 => {
                let pairs = items[1].as_list_or_tuple()?;
                let mut live = names.to_vec();
                if self.values(pairs, &mut live) {
                    return Some(true);
                }
                if head == "when-let" {
                    return Some(self.body(&items[2..], &live));
                }
                let first = pairs.first().and_then(bound).unwrap_or_default();
                let outer = without(names, &first);
                items.get(2).is_some_and(|then| self.reads(then, &live))
                    || items.iter().skip(3).any(|e| self.reads(e, &outer))
            }
            "when-ok" if items.len() >= 2 => {
                let [pattern, value] = items[1].as_list_or_tuple()? else {
                    return None;
                };
                self.reads(value, names) || self.body(&items[2..], &hidden(names, pattern))
            }
            "with" if items.len() >= 4 => {
                let inner = hidden(names, &items[1]);
                self.reads(&items[2], names)
                    || self.reads(&items[3], &inner)
                    || self.body(&items[4..], &inner)
            }
            "with-temp-dir" if items.len() >= 2 => {
                self.body(&items[2..], &hidden(names, &items[1]))
            }
            _ => return None,
        };
        Some(read)
    }
}
