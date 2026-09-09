// audited: 2026-09-08
//! The struct-key half of the layout probe: exemplars and field extents for
//! every `TableKey` variant a dumped struct entry can hold.
//!
//! docs/impl/image/format.md
//!
//! A key is body bytes like an object slot, and it is an enum with padding of
//! its own — a `Bool` key writes one byte into a slot sized for a `Value`. So
//! the dumper assembles an entry's key from these extents rather than copying
//! it, and two dumps of one struct write one file.

use std::mem::size_of;
use std::sync::OnceLock;

use crate::value::{SymbolId, TableKey, Value};

use super::{field_offset, probe, FieldExtent, Probed, VariantLayout};

/// How a key names its own variant. `TableKey` has no tag type of its own,
/// and a probe needs one that is `Copy` and comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyTag {
    Nil,
    Bool,
    Int,
    Symbol,
    String,
    Keyword,
    EmptyList,
    Array,
    Heap,
}

/// Every key variant, which is every key an immutable struct can hold: the
/// dumper refuses a struct whose keys it cannot write, and there is no such
/// key — a key's own value is what the value walk then has to accept.
const PROBED: [KeyTag; 9] = [
    KeyTag::Nil,
    KeyTag::Bool,
    KeyTag::Int,
    KeyTag::Symbol,
    KeyTag::String,
    KeyTag::Keyword,
    KeyTag::EmptyList,
    KeyTag::Array,
    KeyTag::Heap,
];

impl Probed for TableKey {
    type Tag = KeyTag;

    const TAGS: &'static [KeyTag] = &PROBED;
    const PREFIX: &'static str = "key:";
    /// A `Bool` key's payload sits in the byte right after the discriminant,
    /// so a key's discriminant span is one byte rather than a word.
    const DISC_BYTES: usize = 1;

    fn layouts() -> &'static [VariantLayout<KeyTag>] {
        static LAYOUTS: OnceLock<Vec<VariantLayout<KeyTag>>> = OnceLock::new();
        LAYOUTS.get_or_init(probe::<TableKey>)
    }

    fn tag_of(&self) -> KeyTag {
        match self {
            TableKey::Nil => KeyTag::Nil,
            TableKey::Bool(_) => KeyTag::Bool,
            TableKey::Int(_) => KeyTag::Int,
            TableKey::Symbol(_) => KeyTag::Symbol,
            TableKey::String(_) => KeyTag::String,
            TableKey::Keyword(_) => KeyTag::Keyword,
            TableKey::EmptyList => KeyTag::EmptyList,
            TableKey::Array(_) => KeyTag::Array,
            TableKey::Heap(_) => KeyTag::Heap,
        }
    }

    fn exemplar(tag: KeyTag) -> TableKey {
        // A heap-valued exemplar carries a `Value` that names no object: the
        // probe measures where the field sits and never dereferences it.
        let unread = Value::int(3);
        match tag {
            KeyTag::Nil => TableKey::Nil,
            KeyTag::Bool => TableKey::Bool(true),
            KeyTag::Int => TableKey::Int(-1),
            KeyTag::Symbol => TableKey::Symbol(SymbolId::of("image-layout-probe")),
            KeyTag::String => TableKey::String(unread),
            KeyTag::Keyword => TableKey::Keyword(7),
            KeyTag::EmptyList => TableKey::EmptyList,
            KeyTag::Array => TableKey::Array(unread),
            KeyTag::Heap => TableKey::Heap(unread),
        }
    }

    fn extents(&self) -> Vec<FieldExtent> {
        let one =
            |name, at: *const u8, len| vec![FieldExtent::new(name, field_offset(self, at), len)];
        match self {
            TableKey::Nil | TableKey::EmptyList => Vec::new(),
            TableKey::Bool(b) => one("0", b as *const _ as _, size_of::<bool>()),
            TableKey::Int(i) => one("0", i as *const _ as _, size_of::<i64>()),
            TableKey::Symbol(id) => one("0", id as *const _ as _, size_of::<SymbolId>()),
            TableKey::Keyword(h) => one("0", h as *const _ as _, size_of::<u64>()),
            TableKey::String(v) | TableKey::Array(v) | TableKey::Heap(v) => {
                one("0", v as *const _ as _, size_of::<Value>())
            }
        }
    }

    fn fields_intact(original: &TableKey, rebuilt: &TableKey) -> bool {
        match (original, rebuilt) {
            (TableKey::Nil, TableKey::Nil) | (TableKey::EmptyList, TableKey::EmptyList) => true,
            (TableKey::Bool(a), TableKey::Bool(b)) => a == b,
            (TableKey::Int(a), TableKey::Int(b)) => a == b,
            (TableKey::Symbol(a), TableKey::Symbol(b)) => a == b,
            (TableKey::Keyword(a), TableKey::Keyword(b)) => a == b,
            // Compared as raw words rather than as keys: a probe exemplar's
            // `Value` names no object, so `TableKey`'s own comparator — which
            // reads a string key's bytes — has nothing to read.
            (TableKey::String(a), TableKey::String(b))
            | (TableKey::Array(a), TableKey::Array(b))
            | (TableKey::Heap(a), TableKey::Heap(b)) => a.tag == b.tag && a.payload == b.payload,
            _ => false,
        }
    }
}
