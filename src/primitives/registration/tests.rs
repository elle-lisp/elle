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

/// Every primitive in the canonical tables carries an EXAMINED region effect.
///
/// `RegionEffect::Unknown` is the default a table entry gets by omitting
/// `effect:`, and it means "nobody has looked" — the solver then covers the
/// callee with the full may-store arg clique, one never-balancing
/// `IncrefRegion` per heap-argument pair per call
/// (docs/impl/region/effects.md § "What the solver derives"). The census of
/// `Unknown`s over these tables is the declaration work queue, and it is
/// empty; this test keeps it that way, so a new primitive cannot inherit the
/// clique by silence.
///
/// The plugin ABI cannot carry a claim, so plugin-supplied definitions stay
/// `Unknown` by construction — they are not in these tables and are not
/// constrained here.
#[test]
fn every_primitive_declares_an_examined_region_effect() {
    use super::super::def::RegionEffect;
    use super::{ffi_tables, ALL_TABLES};
    let undeclared: Vec<&str> = ALL_TABLES
        .iter()
        .chain(ffi_tables().iter())
        .flat_map(|table| table.iter())
        .filter(|def| def.effect == RegionEffect::Unknown)
        .map(|def| def.name)
        .collect();
    assert!(
        undeclared.is_empty(),
        "primitive(s) left at the RegionEffect::Unknown default: {:?}\n\
         Read the body and declare the strongest claim that holds on EVERY \
         normally-completing path (docs/impl/region/effects.md \"Native region \
         effects: declared, not guessed\") — `Mixed` if none does, `Opaque` if \
         the result is unbounded but no argument is stored.",
        undeclared,
    );
}
