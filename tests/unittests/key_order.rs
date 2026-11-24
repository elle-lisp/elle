// audited: 2026-11-18
// A struct key compares, equates and hashes as the value it holds, so a struct keeps its keys in the one order.
//
// docs/impl/values.md
// docs/types.md

use elle::primitives::ctx::TestHeap;
use elle::value::types::TableKey;
use elle::value::{SymbolId, Value};

// A struct keeps its keys in the one order of all values, so a key compares,
// equates and hashes as the value it holds does. The trap: a key variant that
// ranks by its own declaration order puts a string before a keyword and an
// array before a list pair. A struct then lists its keys in an order `sort`
// does not give them, and an array key ranks its elements a third way.
#[test]
fn a_key_compares_equates_and_hashes_as_the_value_it_holds() {
    use std::hash::{Hash, Hasher};

    fn hash_of<T: Hash>(t: &T) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        t.hash(&mut h);
        h.finish()
    }

    let h = TestHeap::new();
    let c = h.ctx();
    let text = c.string("s");
    let keyword = Value::keyword("k");
    let mut entries = std::collections::BTreeMap::new();
    entries.insert(TableKey::keyword("a"), Value::int(1));
    let values = vec![
        c.set([Value::int(1)].into_iter().collect()),
        c.external("key-order", 1u32),
        c.struct_from(entries),
        c.bytes(vec![1]),
        c.array(vec![text, keyword]),
        c.array(vec![keyword, text]),
        c.array(vec![Value::int(1)]),
        c.pair(Value::int(1), Value::EMPTY_LIST),
        Value::EMPTY_LIST,
        text,
        keyword,
        Value::symbol(SymbolId::of("s")),
        Value::int(1),
        Value::TRUE,
        Value::NIL,
    ];
    let keys: Vec<TableKey> = values
        .iter()
        .map(|v| TableKey::from_value(v).expect("every value here is a valid key"))
        .collect();

    for (i, (ka, va)) in keys.iter().zip(&values).enumerate() {
        assert_eq!(hash_of(ka), hash_of(va), "key {i} hashes as its value");
        for (j, (kb, vb)) in keys.iter().zip(&values).enumerate() {
            assert_eq!(ka.cmp(kb), va.cmp(vb), "key {i} against key {j}");
            assert_eq!(ka == kb, va == vb, "key {i} equated with key {j}");
        }
    }

    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    let mut sorted_values = values.clone();
    sorted_values.sort();
    let key_values: Vec<Value> = sorted_keys.iter().map(|k| k.to_value()).collect();
    assert_eq!(
        key_values, sorted_values,
        "sorted keys hold the values in the order sort gives them"
    );
}
