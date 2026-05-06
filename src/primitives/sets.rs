//! Set primitives for immutable and mutable sets
use std::collections::BTreeSet;

use crate::primitives::collection::{coll_combine, coll_has};
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Recursively freeze mutable values for set insertion.
/// Converts mutable types to immutable equivalents:
/// - @array → array
/// - @struct → struct
/// - mutable-set → set
/// - @string → string (lossy UTF-8)
/// - @bytes → bytes
pub(crate) fn freeze_value(v: Value) -> Value {
    if let Some(arr) = v.as_array_mut() {
        let items: Vec<Value> = arr.borrow().iter().map(|x| freeze_value(*x)).collect();
        Value::array(items)
    } else if let Some(tbl) = v.as_struct_mut() {
        let map: std::collections::BTreeMap<crate::value::TableKey, Value> = tbl
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), freeze_value(*v)))
            .collect();
        Value::struct_from(map)
    } else if let Some(s) = v.as_set_mut() {
        let items: BTreeSet<Value> = s.borrow().iter().map(|x| freeze_value(*x)).collect();
        Value::set(items)
    } else if let Some(buf) = v.as_string_mut() {
        let bytes = buf.borrow().clone();
        Value::string(&*String::from_utf8_lossy(&bytes))
    } else if let Some(blob) = v.as_bytes_mut() {
        let data = blob.borrow().clone();
        Value::bytes(data)
    } else {
        v
    }
}

/// Create an immutable set from elements
///
/// (set elem1 elem2 ...) -> set
///
/// Creates an immutable set, deduplicating elements and freezing mutable values
pub(crate) fn prim_set(args: &[Value]) -> (SignalBits, Value) {
    let mut set = BTreeSet::new();
    for arg in args {
        set.insert(freeze_value(*arg));
    }
    (SIG_OK, Value::set(set))
}

/// Create a mutable set from elements
///
/// (@set elem1 elem2 ...) -> @set
///
/// Creates a mutable set, deduplicating elements and freezing mutable values
pub(crate) fn prim_at_set(args: &[Value]) -> (SignalBits, Value) {
    let mut set = BTreeSet::new();
    for arg in args {
        set.insert(freeze_value(*arg));
    }
    (SIG_OK, Value::set_mut(set))
}

/// Check if a value is a set
///
/// (set? value) -> bool
///
/// Returns true for both immutable and mutable sets. Use (type-of x) to distinguish.
pub(crate) fn prim_is_set(args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::bool(args[0].is_set() || args[0].is_set_mut()),
    )
}

/// Check if a value is in a set, or if a string contains a substring
///
/// (contains? set value) -> bool
/// (contains? string substring) -> bool
///
/// For sets: returns true if the value is a member of the set.
/// For strings: returns true if the string contains the substring.
pub(crate) fn prim_contains(args: &[Value]) -> (SignalBits, Value) {
    match coll_has(&args[0], &args[1]) {
        Ok(found) => (SIG_OK, if found { Value::TRUE } else { Value::FALSE }),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Add an element to a set
///
/// (add set value) -> set
///
/// For immutable sets, returns a new set with the element added.
/// For mutable sets, modifies in place and returns the set.
pub(crate) fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    let frozen = freeze_value(args[1]);
    if let Some(s) = args[0].as_set() {
        let mut new_set: BTreeSet<Value> = s.iter().copied().collect();
        new_set.insert(frozen);
        (SIG_OK, Value::set(new_set))
    } else if let Some(s) = args[0].as_set_mut() {
        s.borrow_mut().insert(frozen);
        (SIG_OK, args[0])
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "add: expected set or mutable set, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Remove an element from a set
///
/// (del set value) -> set
///
/// For immutable sets, returns a new set with the element removed.
/// For mutable sets, modifies in place and returns the set.
pub(crate) fn prim_del(args: &[Value]) -> (SignalBits, Value) {
    let frozen = freeze_value(args[1]);
    if let Some(s) = args[0].as_set() {
        let mut new_set: BTreeSet<Value> = s.iter().copied().collect();
        new_set.remove(&frozen);
        (SIG_OK, Value::set(new_set))
    } else if let Some(s) = args[0].as_set_mut() {
        s.borrow_mut().remove(&frozen);
        (SIG_OK, args[0])
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "del: expected set or mutable set, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Compute the union of two sets
///
/// (union set1 set2) -> set
///
/// Both arguments must be the same type (both immutable or both mutable).
/// Returns a set containing all elements from both sets.
pub(crate) fn prim_union(args: &[Value]) -> (SignalBits, Value) {
    match coll_combine(&args[0], &args[1]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => (SIG_ERROR, e),
    }
}

/// Compute the intersection of two sets
///
/// (intersection set1 set2) -> set
///
/// Both arguments must be the same type (both immutable or both mutable).
/// Returns a set containing only elements present in both sets.
pub(crate) fn prim_intersection(args: &[Value]) -> (SignalBits, Value) {
    if let (Some(a), Some(b)) = (args[0].as_set(), args[1].as_set()) {
        let sa: BTreeSet<Value> = a.iter().copied().collect();
        let sb: BTreeSet<Value> = b.iter().copied().collect();
        let result: BTreeSet<Value> = sa.intersection(&sb).copied().collect();
        (SIG_OK, Value::set(result))
    } else if let (Some(a), Some(b)) = (args[0].as_set_mut(), args[1].as_set_mut()) {
        let result: BTreeSet<Value> = a.borrow().intersection(&*b.borrow()).copied().collect();
        (SIG_OK, Value::set_mut(result))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "intersection: both arguments must be sets or both must be mutable sets"
                    .to_string(),
            ),
        )
    }
}

/// Compute the difference of two sets
///
/// (difference set1 set2) -> set
///
/// Both arguments must be the same type (both immutable or both mutable).
/// Returns a set containing elements in set1 but not in set2.
pub(crate) fn prim_difference(args: &[Value]) -> (SignalBits, Value) {
    if let (Some(a), Some(b)) = (args[0].as_set(), args[1].as_set()) {
        let sa: BTreeSet<Value> = a.iter().copied().collect();
        let sb: BTreeSet<Value> = b.iter().copied().collect();
        let result: BTreeSet<Value> = sa.difference(&sb).copied().collect();
        (SIG_OK, Value::set(result))
    } else if let (Some(a), Some(b)) = (args[0].as_set_mut(), args[1].as_set_mut()) {
        let result: BTreeSet<Value> = a.borrow().difference(&*b.borrow()).copied().collect();
        (SIG_OK, Value::set_mut(result))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "difference: both arguments must be sets or both must be mutable sets".to_string(),
            ),
        )
    }
}

/// Convert a set to an array or @array
///
/// (set->array set) -> array or @array
///
/// Immutable set → array, mutable set → @array. Elements in sorted order.
pub(crate) fn prim_set_to_array(args: &[Value]) -> (SignalBits, Value) {
    if let Some(s) = args[0].as_set() {
        let items: Vec<Value> = s.to_vec();
        (SIG_OK, Value::array(items))
    } else if let Some(s) = args[0].as_set_mut() {
        let items: Vec<Value> = s.borrow().iter().copied().collect();
        (SIG_OK, Value::array_mut(items))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "set->array: expected set or mutable set, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Convert any sequence to a set
///
/// (seq->set seq) -> set or @set
///
/// Immutable inputs (list, array, string, bytes, set) → immutable set.
/// Mutable inputs (@array, @string, @bytes, @set) → mutable set.
/// Mutable values are frozen on insertion.
pub(crate) fn prim_seq_to_set(args: &[Value]) -> (SignalBits, Value) {
    let v = args[0];

    // List (immutable) → immutable set
    if v.is_empty_list() {
        return (SIG_OK, Value::set(BTreeSet::new()));
    }
    if v.as_pair().is_some() {
        let mut set = BTreeSet::new();
        let mut current = v;
        loop {
            if current.is_empty_list() || current.is_nil() {
                break;
            }
            if let Some(pair) = current.as_pair() {
                set.insert(freeze_value(pair.first));
                current = pair.rest;
            } else {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "seq->set: expected proper list, got improper list ending in {}",
                            current.type_name()
                        ),
                    ),
                );
            }
        }
        return (SIG_OK, Value::set(set));
    }

    // Array (immutable) → immutable set
    if let Some(elems) = v.as_array() {
        let set: BTreeSet<Value> = elems.iter().map(|x| freeze_value(*x)).collect();
        return (SIG_OK, Value::set(set));
    }

    // String (immutable) → immutable set of single-grapheme-cluster strings
    if v.is_string() {
        let mut set = BTreeSet::new();
        v.with_string(|s| {
            for ch in s.chars() {
                set.insert(Value::string(ch.to_string()));
            }
        });
        return (SIG_OK, Value::set(set));
    }

    // Bytes (immutable) → immutable set of ints
    if let Some(data) = v.as_bytes() {
        let set: BTreeSet<Value> = data.iter().map(|&b| Value::int(b as i64)).collect();
        return (SIG_OK, Value::set(set));
    }

    // Set (immutable) → immutable set (identity)
    if v.as_set().is_some() {
        return (SIG_OK, v);
    }

    // Array (mutable) → mutable set
    if let Some(arr) = v.as_array_mut() {
        let set: BTreeSet<Value> = arr.borrow().iter().map(|x| freeze_value(*x)).collect();
        return (SIG_OK, Value::set_mut(set));
    }

    // @string (mutable) → mutable set of single-grapheme-cluster strings
    if let Some(buf) = v.as_string_mut() {
        let mut set = BTreeSet::new();
        let bytes = buf.borrow();
        let s = String::from_utf8_lossy(&bytes);
        for ch in s.chars() {
            set.insert(Value::string(ch.to_string()));
        }
        return (SIG_OK, Value::set_mut(set));
    }

    // @bytes (mutable) → mutable set of ints
    if let Some(blob) = v.as_bytes_mut() {
        let set: BTreeSet<Value> = blob
            .borrow()
            .iter()
            .map(|&b| Value::int(b as i64))
            .collect();
        return (SIG_OK, Value::set_mut(set));
    }

    // Mutable set → mutable set (identity)
    if v.as_set_mut().is_some() {
        return (SIG_OK, v);
    }

    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "seq->set: expected sequence (list, array, @array, string, @string, bytes, @bytes, set, @set), got {}",
                v.type_name()
            ),
        ),
    )
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "set",
        func: prim_set,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Create an immutable set from elements (deduplicates, freezes mutable values)",
        params: &[],
        category: "set",
        example: "(set 1 2 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "@set",
        func: prim_at_set,
        signal: Signal::silent(),
        arity: Arity::AtLeast(0),
        doc: "Create a mutable set from elements (deduplicates, freezes mutable values)",
        params: &[],
        category: "set",
        example: "(@set 1 2 3)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "set?",
        func: prim_is_set,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if value is a set (immutable or mutable). Use (type-of x) to distinguish.",
        params: &["value"],
        category: "set",
        example: "(set? (set 1 2)) #=> true\n(set? 42) #=> false",
        aliases: &[],
    },
    // contains? is an alias of has? (defined in lstruct.rs).
    // string-contains? remains registered for old-epoch code; epoch 5 renames it to has?.
    PrimitiveDef {
        name: "string-contains?",
        func: prim_contains,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Deprecated: use has? instead. Check if a string contains a substring.",
        params: &["string", "substring"],
        category: "set",
        example: "(string-contains? \"hello world\" \"world\") #=> true",
        aliases: &[],
    },
    PrimitiveDef {
        name: "add",
        func: prim_add,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Add an element to a set. For immutable sets, returns a new set. For mutable sets, modifies in place.",
        params: &["set", "value"],
        category: "set",
        example: "(add (set 1 2) 3) #=> |1 2 3|",
        aliases: &[],
    },
    PrimitiveDef {
        name: "del",
        func: prim_del,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Remove an element from a set. For immutable sets, returns a new set. For mutable sets, modifies in place.",
        params: &["set", "value"],
        category: "set",
        example: "(del (set 1 2 3) 2) #=> |1 3|",
        aliases: &[],
    },
    PrimitiveDef {
        name: "union",
        func: prim_union,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compute the union of two sets (both must be the same type)",
        params: &["set1", "set2"],
        category: "set",
        example: "(union (set 1 2) (set 2 3)) #=> |1 2 3|",
        aliases: &[],
    },
    PrimitiveDef {
        name: "intersection",
        func: prim_intersection,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compute the intersection of two sets (both must be the same type)",
        params: &["set1", "set2"],
        category: "set",
        example: "(intersection (set 1 2) (set 2 3)) #=> |2|",
        aliases: &[],
    },
    PrimitiveDef {
        name: "difference",
        func: prim_difference,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compute the difference of two sets (both must be the same type)",
        params: &["set1", "set2"],
        category: "set",
        example: "(difference (set 1 2 3) (set 2)) #=> |1 3|",
        aliases: &[],
    },
    PrimitiveDef {
        name: "set->array",
        func: prim_set_to_array,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a set to an array/tuple. Immutable set → tuple, mutable set → array.",
        params: &["set"],
        category: "set",
        example: "(set->array (set 3 1 2)) #=> [1 2 3]",
        aliases: &[],
    },
    PrimitiveDef {
        name: "seq->set",
        func: prim_seq_to_set,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any sequence to a set. Immutable inputs (list, tuple, string, bytes, set) → immutable set. Mutable inputs (array, buffer, blob, @set) → mutable set. Freezes mutable values on insertion.",
        params: &["seq"],
        category: "set",
        example: "(seq->set [1 2 3]) #=> |1 2 3|",
        aliases: &[],
    },
];
