//! Unit tests (`super` is the parent impl module).

use super::super::def::RetType;
use super::def_by_name;

/// Drift guard: type inference's return types live in the registry, so a
/// primitive rename or alias cannot silently desync them. These are the
/// spellings type inference relies on; each must resolve through the
/// registry (name OR alias) to the return type it expects.
#[test]
fn typeinfer_ret_types_resolve_through_registry() {
    let expect = [
        ("array", RetType::Array),
        ("@array", RetType::MutableArray),
        ("struct", RetType::Struct),
        ("@struct", RetType::MutableStruct),
        ("string", RetType::String),
        ("length", RetType::Int),
        ("type", RetType::Keyword),
        ("has?", RetType::Bool),
        ("empty?", RetType::Bool),
        ("contains?", RetType::Bool),
        ("ptr?", RetType::Bool),
        ("pointer?", RetType::Bool),
        ("string/contains?", RetType::Bool),
        ("string-contains?", RetType::Bool),
        ("string/starts-with?", RetType::Bool),
        ("string-starts-with?", RetType::Bool),
        ("string/ends-with?", RetType::Bool),
        ("string-ends-with?", RetType::Bool),
        ("number->string", RetType::String),
    ];
    for (name, ret) in expect {
        let def = def_by_name(name)
            .unwrap_or_else(|| panic!("primitive '{}' missing from registry index", name));
        assert_eq!(
            def.ret, ret,
            "primitive '{}' (def '{}') return type drifted",
            name, def.name
        );
    }
    // `push`/`put` are stdlib.lisp closures, not primitives — they must
    // NOT appear in the registry (typeinfer special-cases them).
    assert!(
        def_by_name("push").is_none(),
        "push became a primitive — move its typing to the registry"
    );
    assert!(
        def_by_name("put").is_none(),
        "put became a primitive — move its typing to the registry"
    );
}
