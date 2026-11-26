// audited: 2026-11-25
//! What every primitive table entry must hold: its return type, region effect, signal, and claim to run no Elle code.
//!
//! docs/impl/region/effects.md
//! docs/signals/inference.md
//! docs/impl/typeinfer/facts.md

use super::super::def::RetType;
use super::def_by_name;

mod spelling;

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
/// (docs/impl/region/effects.md). The census of
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
    let undeclared: Vec<&str> = super::canonical_defs()
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

/// A primitive declared silent raises nothing, whatever it is handed.
///
/// A silent signal is a proof the compiler builds on: a function that calls
/// only silent primitives infers silent, and a raise that leaves it is a broken
/// proof (docs/signals/inference.md). So each silent primitive is called with
/// arguments of every wrong type, at its fewest arguments and at one more where
/// it takes one more. The `%` intrinsics are exempt by design: a call that names
/// one compiles only when its operands are proven, and a call through a value
/// reaches it as a callee the compiler does not know.
///
/// The counter-factual is an entry that omits `signal:`. It takes the silent
/// default, and a primitive that checks its argument's type then breaks the
/// proof of every function that calls it.
#[test]
fn a_primitive_declared_silent_raises_nothing() {
    use crate::primitives::ctx::with_test_ctx_keep_region;
    use crate::signals::Signal;
    use crate::value::{Arity, Value, SIG_ERROR};

    let junk = [Value::int(42), Value::NIL, Value::TRUE, Value::float(1.5)];
    let mut raising = Vec::new();
    for def in super::canonical_defs() {
        if def.signal != Signal::silent() || def.name.starts_with('%') {
            continue;
        }
        let fewest = match def.arity {
            Arity::Exact(n) | Arity::AtLeast(n) | Arity::Range(n, _) => n,
        };
        let counts = [fewest, fewest + 1]
            .into_iter()
            .filter(|n| def.arity.matches(*n));
        for count in counts {
            for value in junk {
                let args = vec![value; count];
                let (bits, _) = with_test_ctx_keep_region(|ctx| (def.func)(ctx, &args));
                if bits.intersects(SIG_ERROR) {
                    raising.push(format!("{} with {} × {}", def.name, count, value));
                }
            }
        }
    }
    assert!(
        raising.is_empty(),
        "primitive(s) declared silent that raise: {raising:?}\nDeclare the \
         signal the body can raise, `Signal::errors()` for a type check."
    );
}

/// The primitives declared to run no Elle code are `type-of` and the type
/// predicates the runtime supplies, each a test of its argument's tag. Type
/// inference keeps a guard's fact across a call to one
/// (docs/impl/typeinfer/facts.md). The counter-factual is a table where no
/// entry declares it, so every such call ends the facts about a shared binding.
#[test]
fn the_primitives_that_run_no_elle_are_the_type_tests() {
    let mut declared: Vec<&str> = super::canonical_defs()
        .filter(|def| def.runs_no_elle)
        .map(|def| def.name)
        .collect();
    declared.sort_unstable();
    let expected = [
        "fiber?",
        "io-backend?",
        "io-request?",
        "port?",
        "ptr?",
        "subprocess?",
        "type-of",
    ];
    assert_eq!(declared, expected);
}

/// A primitive that declares it runs no Elle code returns no signal the
/// dispatch loop answers by running code or by parking the fiber. The
/// counter-factual is `fiber/resume` declared so: its `:resume` runs the
/// fiber, which may write a binding the caller's fact is about.
#[test]
fn the_primitives_that_run_no_elle_signal_nothing_that_runs_code() {
    use crate::signals::{
        SIG_FFI, SIG_IMPORT, SIG_IO, SIG_QUERY, SIG_RESUME, SIG_SWITCH, SIG_WAIT, SIG_YIELD,
    };
    let runs = SIG_RESUME
        .union(SIG_YIELD)
        .union(SIG_QUERY)
        .union(SIG_SWITCH)
        .union(SIG_IO)
        .union(SIG_WAIT)
        .union(SIG_IMPORT)
        .union(SIG_FFI);
    for def in super::canonical_defs().filter(|def| def.runs_no_elle) {
        assert!(
            !def.signal.bits.intersects(runs),
            "{} declares it runs no Elle code, and its signal can run code",
            def.name
        );
    }
}
