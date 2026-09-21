// audited: 2026-09-21
//! Assemble a booted instance's boot state as one immutable struct, the root
//! a boot image is dumped from.
//!
//! docs/impl/image/boot.md
//!
//! Every field is ordinary sealed data, so the ordinary dumper carries it. The
//! aggregate is built in a scratch region and names values the instance owns
//! without taking a reference to any of them: the dump reads through it, and
//! the region goes when the dump is done.

use std::collections::BTreeMap;

use crate::hir::region::RuntimeRegion;
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::{build, TableKey, Value};

use super::super::ImageError;
use super::{key, sources_digest};

/// The boot root struct: the source digest, the two export structs, and the
/// macro table.
pub(super) fn root(
    heap: &mut FiberHeap,
    symbols: &mut SymbolTable,
    cctx: &CompileCtx,
    region: RuntimeRegion,
) -> Result<Value, ImageError> {
    let exports = cctx.boot_exports();
    if exports.core.as_struct().is_none() {
        return Err(ImageError::Unsupported(
            "this instance has no core exports to dump".into(),
        ));
    }
    if exports.stdlib.as_struct().is_none() {
        return Err(ImageError::Unsupported(
            "this instance has no stdlib exports to dump".into(),
        ));
    }
    let digest = build::string(heap, sources_digest(), region);
    let macros = macro_table(heap, symbols, cctx, region);
    Ok(structure(
        heap,
        symbols,
        &[
            (key::SOURCES, digest),
            (key::CORE, exports.core),
            (key::STDLIB, exports.stdlib),
            (key::MACROS, macros),
        ],
        region,
    ))
}

/// The macro table: one entry per macro, keyed by the macro's name.
fn macro_table(
    heap: &mut FiberHeap,
    symbols: &mut SymbolTable,
    cctx: &CompileCtx,
    region: RuntimeRegion,
) -> Value {
    // The names come off a hash map, so they are sorted here rather than
    // wherever the map happened to put them: one boot state must write one
    // file (docs/impl/image.md § Dumping).
    let mut names: Vec<String> = cctx.macros().keys().cloned().collect();
    names.sort();
    let mut entries: Vec<(&str, Value)> = Vec::with_capacity(names.len());
    for name in &names {
        let def = &cctx.macros()[name];
        let params = strings(heap, &def.params, region);
        let optional = strings(heap, &def.optional_params, region);
        let rest = match &def.rest_param {
            Some(r) => build::string(heap, r, region),
            None => Value::NIL,
        };
        let template = build::syntax(heap, def.template, region);
        let transformer = def.cached_transformer.borrow().unwrap_or(Value::NIL);
        let entry = structure(
            heap,
            symbols,
            &[
                (key::PARAMS, params),
                (key::OPTIONAL, optional),
                (key::REST, rest),
                (key::TEMPLATE, template),
                (key::TRANSFORMER, transformer),
            ],
            region,
        );
        entries.push((name.as_str(), entry));
    }
    structure(heap, symbols, &entries, region)
}

/// An immutable struct over keyword keys, with each key's spelling recorded in
/// the memo the dump reads its name table out of.
fn structure(
    heap: &mut FiberHeap,
    symbols: &mut SymbolTable,
    entries: &[(&str, Value)],
    region: RuntimeRegion,
) -> Value {
    let mut fields: BTreeMap<TableKey, Value> = BTreeMap::new();
    for (name, value) in entries {
        // A keyword payload is its name's hash, so the spelling is what has to
        // travel; recording it here is what puts it in the image's name table.
        symbols.keyword(name);
        fields.insert(TableKey::keyword(name), *value);
    }
    build::struct_from(heap, fields, region)
}

/// An immutable array of strings.
fn strings(heap: &mut FiberHeap, items: &[String], region: RuntimeRegion) -> Value {
    let values: Vec<Value> = items
        .iter()
        .map(|s| build::string(heap, s, region))
        .collect();
    build::array(heap, values, region)
}
