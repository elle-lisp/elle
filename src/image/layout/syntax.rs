// audited: 2026-09-09
//! The syntax half of the layout probe: exemplars and field extents for every
//! `SyntaxKind`, and the writer for the node that wraps one.
//!
//! docs/impl/image/format.md
//! docs/impl/syntax.md
//!
//! A node is the third record the dumper writes into page bytes, and the least
//! plain of the three: a struct holding an enum, a 20-byte span, an inline
//! slice, and a `bool`. Copying one would carry its construction temporary's
//! padding into the artifact, so a node is assembled from these extents like
//! an object slot and a struct entry.

use std::mem::{offset_of, size_of};
use std::sync::OnceLock;

use crate::syntax::{ScopeId, SynRef, Syntax, SyntaxKind};
use crate::value::region_slice::{RegionSlice, RegionStr};

use super::{field_offset, probe, write_canonical, FieldExtent, Probed, VariantLayout};

/// How a node names its kind. `SyntaxKind` has no tag type of its own, and a
/// probe needs one that is `Copy` and comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KindTag {
    Nil,
    Bool,
    Int,
    Float,
    Symbol,
    Keyword,
    Str,
    StrMut,
    List,
    Array,
    ArrayMut,
    Struct,
    StructMut,
    Set,
    SetMut,
    Bytes,
    BytesMut,
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    Splice,
    SyntaxLiteral,
}

/// Every kind, which is every kind a tree can hold: a node the dumper met and
/// could not write would be a tree it had to refuse whole.
const PROBED: [KindTag; 23] = [
    KindTag::Nil,
    KindTag::Bool,
    KindTag::Int,
    KindTag::Float,
    KindTag::Symbol,
    KindTag::Keyword,
    KindTag::Str,
    KindTag::StrMut,
    KindTag::List,
    KindTag::Array,
    KindTag::ArrayMut,
    KindTag::Struct,
    KindTag::StructMut,
    KindTag::Set,
    KindTag::SetMut,
    KindTag::Bytes,
    KindTag::BytesMut,
    KindTag::Quote,
    KindTag::Quasiquote,
    KindTag::Unquote,
    KindTag::UnquoteSplicing,
    KindTag::Splice,
    KindTag::SyntaxLiteral,
];

impl Probed for SyntaxKind {
    type Tag = KindTag;

    const TAGS: &'static [KindTag] = &PROBED;
    const PREFIX: &'static str = "kind:";
    /// A `Bool` kind's payload sits in the byte right after the discriminant,
    /// as a `TableKey`'s does.
    const DISC_BYTES: usize = 1;

    fn layouts() -> &'static [VariantLayout<KindTag>] {
        static LAYOUTS: OnceLock<Vec<VariantLayout<KindTag>>> = OnceLock::new();
        LAYOUTS.get_or_init(probe::<SyntaxKind>)
    }

    fn tag_of(&self) -> KindTag {
        use SyntaxKind::*;
        match self {
            Nil => KindTag::Nil,
            Bool(_) => KindTag::Bool,
            Int(_) => KindTag::Int,
            Float(_) => KindTag::Float,
            Symbol(_) => KindTag::Symbol,
            Keyword(_) => KindTag::Keyword,
            String(_) => KindTag::Str,
            StringMut(_) => KindTag::StrMut,
            List(_) => KindTag::List,
            Array(_) => KindTag::Array,
            ArrayMut(_) => KindTag::ArrayMut,
            Struct(_) => KindTag::Struct,
            StructMut(_) => KindTag::StructMut,
            Set(_) => KindTag::Set,
            SetMut(_) => KindTag::SetMut,
            Bytes(_) => KindTag::Bytes,
            BytesMut(_) => KindTag::BytesMut,
            Quote(_) => KindTag::Quote,
            Quasiquote(_) => KindTag::Quasiquote,
            Unquote(_) => KindTag::Unquote,
            UnquoteSplicing(_) => KindTag::UnquoteSplicing,
            Splice(_) => KindTag::Splice,
            SyntaxLiteral(_) => KindTag::SyntaxLiteral,
        }
    }

    fn exemplar(tag: KindTag) -> SyntaxKind {
        // A wrapping kind's exemplar names no node: the probe measures where
        // the pointer sits and never follows it.
        let unread = unsafe { SynRef::from_raw(std::ptr::NonNull::<Syntax>::dangling().as_ptr()) };
        let text = RegionStr::empty();
        let nodes = RegionSlice::<Syntax>::empty();
        use SyntaxKind::*;
        match tag {
            KindTag::Nil => Nil,
            KindTag::Bool => Bool(true),
            KindTag::Int => Int(-1),
            KindTag::Float => Float(1.5),
            KindTag::Symbol => Symbol(text),
            KindTag::Keyword => Keyword(text),
            KindTag::Str => String(text),
            KindTag::StrMut => StringMut(text),
            KindTag::List => List(nodes),
            KindTag::Array => Array(nodes),
            KindTag::ArrayMut => ArrayMut(nodes),
            KindTag::Struct => Struct(nodes),
            KindTag::StructMut => StructMut(nodes),
            KindTag::Set => Set(nodes),
            KindTag::SetMut => SetMut(nodes),
            KindTag::Bytes => Bytes(nodes),
            KindTag::BytesMut => BytesMut(nodes),
            KindTag::Quote => Quote(unread),
            KindTag::Quasiquote => Quasiquote(unread),
            KindTag::Unquote => Unquote(unread),
            KindTag::UnquoteSplicing => UnquoteSplicing(unread),
            KindTag::Splice => Splice(unread),
            KindTag::SyntaxLiteral => SyntaxLiteral(unread),
        }
    }

    fn extents(&self) -> Vec<FieldExtent> {
        use SyntaxKind::*;
        let scalar = |at: *const u8, len| vec![FieldExtent::new("0", field_offset(self, at), len)];
        match self {
            Nil => Vec::new(),
            Bool(b) => scalar(b as *const _ as _, size_of::<bool>()),
            Int(i) => scalar(i as *const _ as _, size_of::<i64>()),
            Float(f) => scalar(f as *const _ as _, size_of::<f64>()),
            Symbol(s) | Keyword(s) | String(s) | StringMut(s) => {
                slice_extents(field_offset(self, s as *const _ as _))
            }
            List(n) | Array(n) | ArrayMut(n) | Struct(n) | StructMut(n) | Set(n) | SetMut(n)
            | Bytes(n) | BytesMut(n) => slice_extents(field_offset(self, n as *const _ as _)),
            Quote(r) | Quasiquote(r) | Unquote(r) | UnquoteSplicing(r) | Splice(r)
            | SyntaxLiteral(r) => scalar(r as *const _ as _, size_of::<*const Syntax>()),
        }
    }

    fn fields_intact(original: &SyntaxKind, rebuilt: &SyntaxKind) -> bool {
        use SyntaxKind::*;
        match (original, rebuilt) {
            (Nil, Nil) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a.to_bits() == b.to_bits(),
            (Symbol(a), Symbol(b))
            | (Keyword(a), Keyword(b))
            | (String(a), String(b))
            | (StringMut(a), StringMut(b)) => {
                a.bytes().as_ptr() == b.bytes().as_ptr() && a.len() == b.len()
            }
            (List(a), List(b))
            | (Array(a), Array(b))
            | (ArrayMut(a), ArrayMut(b))
            | (Struct(a), Struct(b))
            | (StructMut(a), StructMut(b))
            | (Set(a), Set(b))
            | (SetMut(a), SetMut(b))
            | (Bytes(a), Bytes(b))
            | (BytesMut(a), BytesMut(b)) => a.as_ptr() == b.as_ptr() && a.len() == b.len(),
            // Compared as addresses: a probe exemplar's reference names no
            // node, so there is nothing behind it to compare.
            (Quote(a), Quote(b))
            | (Quasiquote(a), Quasiquote(b))
            | (Unquote(a), Unquote(b))
            | (UnquoteSplicing(a), UnquoteSplicing(b))
            | (Splice(a), Splice(b))
            | (SyntaxLiteral(a), SyntaxLiteral(b)) => a.as_ptr() == b.as_ptr(),
            _ => false,
        }
    }
}

/// The two extents of a slice payload at `base`, whatever its element type —
/// every `RegionSlice` header is laid out alike, which `assert_nested_layout`
/// checks.
fn slice_extents(base: usize) -> Vec<FieldExtent> {
    let (ptr, len, len_size) = RegionSlice::<u8>::header_layout();
    vec![
        FieldExtent::new("0.ptr", base + ptr, size_of::<*const u8>()),
        FieldExtent::new("0.len", base + len, len_size),
    ]
}

/// Where a node keeps each of its four fields, measured once.
fn node_offsets() -> &'static [usize; 4] {
    static OFFSETS: OnceLock<[usize; 4]> = OnceLock::new();
    OFFSETS.get_or_init(|| {
        // A span is five `u32`s with nothing between them, so one extent
        // covers it. The image would otherwise have to probe a fourth record.
        assert_eq!(
            size_of::<crate::syntax::Span>(),
            5 * size_of::<u32>(),
            "image layout probe: Span has padding"
        );
        [
            offset_of!(Syntax, kind),
            offset_of!(Syntax, span),
            offset_of!(Syntax, scopes),
            offset_of!(Syntax, scope_exempt),
        ]
    })
}

/// Copy one syntax node's canonical bytes into the zeroed slot `dst`: its
/// kind through the kind probe, its span whole, its scope slice's two header
/// fields, and its flag byte. Everything else in the slot stays zero.
pub(crate) fn write_canonical_node(node: &Syntax, dst: &mut [u8]) {
    debug_assert_eq!(dst.len(), size_of::<Syntax>());
    let [kind_at, span_at, scopes_at, flag_at] = *node_offsets();
    write_canonical(
        &node.kind,
        &mut dst[kind_at..kind_at + size_of::<SyntaxKind>()],
    );
    let span = unsafe {
        std::slice::from_raw_parts(
            &node.span as *const _ as *const u8,
            size_of::<crate::syntax::Span>(),
        )
    };
    dst[span_at..span_at + span.len()].copy_from_slice(span);
    let (ptr_at, len_at, len_size) = RegionSlice::<ScopeId>::header_layout();
    let scopes = unsafe {
        std::slice::from_raw_parts(
            &node.scopes as *const _ as *const u8,
            size_of::<RegionSlice<ScopeId>>(),
        )
    };
    dst[scopes_at + ptr_at..scopes_at + ptr_at + size_of::<*const u8>()]
        .copy_from_slice(&scopes[ptr_at..ptr_at + size_of::<*const u8>()]);
    dst[scopes_at + len_at..scopes_at + len_at + len_size]
        .copy_from_slice(&scopes[len_at..len_at + len_size]);
    dst[flag_at] = node.scope_exempt as u8;
}

/// Where a node keeps the file id hydration rewrites in place: the span's
/// offset in the node plus the id's offset in the span.
pub(crate) fn file_slot_in_node() -> usize {
    node_offsets()[1] + crate::syntax::Span::file_offset()
}

/// The fingerprint's syntax section: where a node keeps its fields, and where
/// a span keeps the file id the image rewrites in place.
pub(crate) fn fingerprint_component() -> String {
    let [kind_at, span_at, scopes_at, flag_at] = *node_offsets();
    format!(
        "node:kind@{kind_at},span@{span_at},scopes@{scopes_at},exempt@{flag_at};span:file@{}",
        crate::syntax::Span::file_offset()
    )
}
