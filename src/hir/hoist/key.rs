// audited: 2026-11-17
//! The key two constants share exactly when they hold the same contents, a float compared by its bits.
//!
//! docs/impl/hoist.md
//! docs/types.md

use crate::hir::expr::{Hir, HirKind};

/// A constant's contents as bytes. Two constants with one key hold contents
/// that `identical?` cannot tell apart. Each part writes a tag and, where its
/// size varies, its length, so no two constants of different shapes write the
/// same bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ConstKey(Vec<u8>);

impl ConstKey {
    /// The key of `hir`, which the hoist found to be a constant: a string, an
    /// immutable quoted datum, or a collection literal over immediates and
    /// constants.
    pub(super) fn of(hir: &Hir) -> ConstKey {
        let mut bytes = Vec::new();
        write(hir, &mut bytes);
        ConstKey(bytes)
    }
}

/// The tags of the key, one per form a constant or an item of one takes.
mod tag {
    pub(super) const STRING: u8 = 0;
    pub(super) const QUOTED: u8 = 1;
    pub(super) const COLLECTION: u8 = 2;
    pub(super) const NIL: u8 = 3;
    pub(super) const EMPTY_LIST: u8 = 4;
    pub(super) const BOOL: u8 = 5;
    pub(super) const INT: u8 = 6;
    pub(super) const FLOAT: u8 = 7;
    pub(super) const KEYWORD: u8 = 8;
    pub(super) const VALUE: u8 = 9;
}

fn write(hir: &Hir, out: &mut Vec<u8>) {
    match &hir.kind {
        HirKind::String(s) => text(tag::STRING, s, out),
        HirKind::QuoteConst(template) => {
            // The template's own encoding writes a float by its bits and a
            // symbol, a syntax symbol's scopes and span included, by its name.
            let mut encoded = Vec::new();
            template.encode(&mut encoded);
            out.push(tag::QUOTED);
            out.extend_from_slice(&(encoded.len() as u64).to_be_bytes());
            out.extend_from_slice(&encoded);
        }
        HirKind::Call { func, args, .. } => {
            let HirKind::Var(ctor) = &func.kind else {
                unreachable!("a collection literal calls its constructor by name")
            };
            out.push(tag::COLLECTION);
            // One binding names each primitive in a unit, so the binding is the
            // constructor's identity.
            out.extend_from_slice(&ctor.0.to_be_bytes());
            out.extend_from_slice(&(args.len() as u64).to_be_bytes());
            for arg in args {
                write(&arg.expr, out);
            }
        }
        HirKind::Nil => out.push(tag::NIL),
        HirKind::EmptyList => out.push(tag::EMPTY_LIST),
        HirKind::Bool(b) => out.extend_from_slice(&[tag::BOOL, u8::from(*b)]),
        HirKind::Int(n) => {
            out.push(tag::INT);
            out.extend_from_slice(&n.to_be_bytes());
        }
        HirKind::Float(f) => {
            out.push(tag::FLOAT);
            out.extend_from_slice(&f.to_bits().to_be_bytes());
        }
        HirKind::Keyword(k) => text(tag::KEYWORD, k, out),
        // An immediate, or on the macro path a syntax object: its 16 bytes
        // are its identity either way.
        HirKind::Quote(v) => {
            out.push(tag::VALUE);
            out.extend_from_slice(&v.tag.to_be_bytes());
            out.extend_from_slice(&v.payload.to_be_bytes());
        }
        other => unreachable!("no constant holds {other:?}"),
    }
}

fn text(tag: u8, s: &str, out: &mut Vec<u8>) {
    out.push(tag);
    out.extend_from_slice(&(s.len() as u64).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
}
