//! Unit tests for the JSON serializer's keyword handling.
//!
//! A keyword IS a name hash; the spelling comes from the calling instance's
//! memo or from the static vocabulary (docs/impl/symbol.md). JSON has no
//! rendering for a hash — `:0xcbf2…` would read back as a different name — so
//! a spelling neither source carries is a refusal, and the refusal has to say
//! which value it could not spell. Everything else in this module is covered
//! by `tests/elle/prim-json.lisp` and `tests/elle/keyword-spelling.lisp`.

use super::{serialize_value, serialize_value_pretty};
use crate::symbol::SymbolTable;
use crate::value::{TableKey, Value};

/// A spelling no vocabulary entry and no other test carries.
const UNLEARNED: &str = "json-unspelled-keyword-xt";

fn struct_with_keyword_key(name: &str) -> Value {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let heap = unsafe { &mut *heap_ptr };
    let region = heap.new_runtime_region();
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(TableKey::keyword(name), Value::int(1));
    crate::value::build::struct_from(heap, fields, region)
}

// The refusal names the value. "has no learned spelling" alone leaves an
// author with a failing `json/serialize` and nothing to search for; the hash
// is the one handle on which keyword it was.
#[test]
fn an_unspellable_keyword_value_is_refused_by_its_hash() {
    let hash = crate::value::keyword::keyword_hash(UNLEARNED);
    let err = serialize_value(&Value::keyword(UNLEARNED), None)
        .expect_err("a keyword with no spelling cannot be written as a JSON string");
    assert!(
        err.contains(&format!("{:#x}", hash)),
        "the refusal must name the hash it could not spell, got: {err}"
    );
}

#[test]
fn an_unspellable_struct_key_is_refused_by_its_hash() {
    let hash = crate::value::keyword::keyword_hash(UNLEARNED);
    let value = struct_with_keyword_key(UNLEARNED);

    let err = serialize_value(&value, None)
        .expect_err("a struct key with no spelling cannot be written as a JSON key");
    assert!(
        err.contains(&format!("{:#x}", hash)),
        "the compact refusal must name the hash it could not spell, got: {err}"
    );

    let err = serialize_value_pretty(&value, None, 0)
        .expect_err("a struct key with no spelling cannot be written as a JSON key");
    assert!(
        err.contains(&format!("{:#x}", hash)),
        "the pretty refusal must name the hash it could not spell, got: {err}"
    );
}

// The counter-factual for both refusals above: the same value, the same
// serializer, and a memo that met the name. Nothing about the value changed —
// only what the instance knows — so a refusal here would mean the serializer
// never consults the memo it is handed.
#[test]
fn a_learned_struct_key_writes_as_its_spelling() {
    let mut memo = SymbolTable::new();
    memo.keyword(UNLEARNED);
    let value = struct_with_keyword_key(UNLEARNED);

    assert_eq!(
        serialize_value(&value, Some(&memo)).expect("a learned key writes"),
        format!("{{\"{UNLEARNED}\":1}}")
    );
    assert_eq!(
        serialize_value(&Value::keyword(UNLEARNED), Some(&memo)).expect("a learned keyword writes"),
        format!("\"{UNLEARNED}\"")
    );
}
