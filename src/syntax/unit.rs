// audited: 2026-10-14
//! A unit of expansion: the tag that names it, and the counts its scope ids and gensym names take.
//!
//! docs/impl/syntax.md
//! docs/macros.md
//!
//! A unit is what one compile expands. Its tag hashes the unit's source name
//! and its forms as read, so a unit expands to the same ids and names in every
//! process, and two units to different ones.

use std::fmt;

use super::{Syntax, SyntaxKind};
use crate::namehash::Fnv1a;

/// The tag of a unit of expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitTag(pub(super) u32);

impl UnitTag {
    /// The tag no unit takes: the primitive scope and the compiler's intro
    /// scope carry it.
    pub(crate) const NONE: UnitTag = UnitTag(0);

    /// The tag of the unit `name` names, whose forms are `forms` as read.
    ///
    /// The hash reads each node's kind, atoms and spellings, and neither its
    /// span nor its scopes, so the same forms set out differently in a file
    /// are the same unit. A hash that folds to zero takes tag one, because
    /// zero is [`NONE`](Self::NONE).
    pub(crate) fn of(name: &str, forms: &[Syntax]) -> UnitTag {
        let mut hash = Fnv1a::new();
        hash.write_str(name);
        hash.write_len(forms.len());
        for form in forms {
            write_node(&mut hash, form);
        }
        let wide = hash.finish();
        let tag = UnitTag((wide ^ (wide >> 32)) as u32);
        if tag == Self::NONE {
            UnitTag(1)
        } else {
            tag
        }
    }
}

impl fmt::LowerHex for UnitTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::LowerHex::fmt(&self.0, f)
    }
}

fn write_spelling(hash: &mut Fnv1a, kind: u8, s: &str) {
    hash.write(&[kind]);
    hash.write_str(s);
}

/// Feed one node to `hash`: a byte for its kind, then its atom, its spelling,
/// or its children.
fn write_node(hash: &mut Fnv1a, node: &Syntax) {
    use SyntaxKind::*;
    let (kind, children): (u8, &[Syntax]) = match &node.kind {
        Nil => (0, &[]),
        Bool(b) => {
            hash.write(&[1, *b as u8]);
            return;
        }
        Int(n) => {
            hash.write(&[2]);
            hash.write(&n.to_le_bytes());
            return;
        }
        Float(f) => {
            hash.write(&[3]);
            hash.write(&f.to_bits().to_le_bytes());
            return;
        }
        Symbol(s) => return write_spelling(hash, 4, s),
        Keyword(s) => return write_spelling(hash, 5, s),
        String(s) => return write_spelling(hash, 6, s),
        StringMut(s) => return write_spelling(hash, 7, s),
        List(items) => (8, items),
        Array(items) => (9, items),
        ArrayMut(items) => (10, items),
        Struct(items) => (11, items),
        StructMut(items) => (12, items),
        Set(items) => (13, items),
        SetMut(items) => (14, items),
        Bytes(items) => (15, items),
        BytesMut(items) => (16, items),
        Quote(inner) => (17, std::slice::from_ref(&**inner)),
        Quasiquote(inner) => (18, std::slice::from_ref(&**inner)),
        Unquote(inner) => (19, std::slice::from_ref(&**inner)),
        UnquoteSplicing(inner) => (20, std::slice::from_ref(&**inner)),
        Splice(inner) => (21, std::slice::from_ref(&**inner)),
        SyntaxLiteral(inner) => (22, std::slice::from_ref(&**inner)),
    };
    hash.write(&[kind]);
    hash.write_len(children.len());
    for child in children {
        write_node(hash, child);
    }
}

/// What an expander counts in the unit it expands: the scopes it has minted,
/// and the names `gensym` has answered.
///
/// A transformer call lends a copy to the VM, where `gensym` reads it, and
/// [`take_back`](Self::take_back) keeps what the call ran up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unit {
    tag: UnitTag,
    scopes: u32,
    gensyms: u64,
}

impl Unit {
    /// The unit `tag` names, before it has minted anything.
    pub(crate) fn begin(tag: UnitTag) -> Unit {
        Unit {
            tag,
            scopes: 0,
            gensyms: 0,
        }
    }

    pub(crate) fn tag(self) -> UnitTag {
        self.tag
    }

    /// The count of the next scope this unit mints.
    pub(crate) fn next_scope(&mut self) -> u32 {
        let count = self.scopes;
        self.scopes += 1;
        count
    }

    /// The name `gensym` answers for `prefix` while this unit expands: the
    /// prefix, the unit's count of names, a hyphen and the tag in hex.
    pub(crate) fn gensym(&mut self, prefix: &str) -> String {
        let count = self.gensyms;
        self.gensyms += 1;
        format!("{prefix}{count}-{:08x}", self.tag)
    }

    /// Keep the names a copy of this unit, lent to the VM, answered.
    pub(crate) fn take_back(&mut self, lent: Unit) {
        debug_assert_eq!(lent.tag, self.tag, "a unit took back another's copy");
        self.gensyms = lent.gensyms;
    }
}
