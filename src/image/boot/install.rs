// audited: 2026-09-21
//! Read a hydrated boot image's macro table back into an expander.
//!
//! docs/impl/image/boot.md
//!
//! A macro entry is a struct, so reading one is four field lookups and a
//! borrow of the tree it names. The tree stays where it is: the hydrated
//! region is a process root, so it outlives every expansion that reads it.

use std::cell::RefCell;
use std::rc::Rc;

use crate::symbol::SymbolTable;
use crate::syntax::{Expander, MacroDef};
use crate::value::fiberheap::FiberHeap;
use crate::value::{TableKey, Value};

use super::super::ImageError;
use super::{field, field_text, key, Boot};

/// Every macro name the image carries has a learned spelling.
///
/// The install reads each name out of the hydrating instance's memo, which the
/// image's name table has just been replayed into. A missing spelling means a
/// table that does not cover its own keys, so it is a refusal here rather than
/// a panic during the install.
pub(super) fn check_names(boot: &Boot, symbols: &SymbolTable) -> Result<(), ImageError> {
    for (key, _) in boot.macro_entries() {
        let TableKey::Keyword(hash) = key else {
            return Err(ImageError::Corrupt(
                "a boot image's macro table is keyed by something other than a name".into(),
            ));
        };
        if symbols.keyword_name(*hash).is_none() {
            return Err(ImageError::Corrupt(format!(
                "a boot image's macro table names {hash:#x}, whose spelling its name table omits"
            )));
        }
    }
    Ok(())
}

/// Define every macro the image carries on `expander`.
///
/// Each filled transformer cell takes a reference to the region its closure
/// lives in — the image's own — because the teardown that empties these cells
/// releases one per cell (`Expander::release_cached_transformers`). Without the
/// reference here that release would decref the whole boot graph once per
/// macro.
pub(super) fn macros(
    heap: &mut FiberHeap,
    boot: &Boot,
    expander: &mut Expander,
    symbols: &SymbolTable,
) {
    // The names are read out before the definitions are built, because
    // building one borrows the heap and reading one borrows the image.
    let entries: Vec<(String, Value)> = boot
        .macro_entries()
        .iter()
        .map(|(key, entry)| {
            let TableKey::Keyword(hash) = key else {
                unreachable!("check_names refused a table keyed by anything else");
            };
            let name = symbols
                .keyword_name(*hash)
                .expect("check_names refused a name with no spelling");
            (name.to_owned(), *entry)
        })
        .collect();
    for (name, entry) in entries {
        if let Some(def) = macro_def(heap, &name, entry) {
            expander.define_macro(def);
        }
    }
}

/// One macro definition out of its entry struct.
fn macro_def(heap: &mut FiberHeap, name: &str, entry: Value) -> Option<MacroDef> {
    let tree = field(entry, key::TEMPLATE)?;
    let template = *tree.as_syntax()?;
    let transformer = field(entry, key::TRANSFORMER).filter(|v| !v.is_nil());
    if let Some(v) = transformer {
        let region = crate::value::arena::region_of(heap, v);
        crate::value::arena::incref_region(heap, region);
    }
    Some(MacroDef {
        name: name.to_string(),
        params: strings(entry, key::PARAMS),
        optional_params: strings(entry, key::OPTIONAL),
        rest_param: field_text(entry, key::REST),
        template,
        cached_transformer: Rc::new(RefCell::new(transformer)),
    })
}

/// A parameter list: the strings of the array the entry holds under `name`.
fn strings(entry: Value, name: &str) -> Vec<String> {
    let Some(array) = field(entry, name) else {
        return Vec::new();
    };
    let Some(items) = array.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}
