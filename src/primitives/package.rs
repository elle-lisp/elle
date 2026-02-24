use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::{list, Value};

/// Get the current package version
pub fn prim_package_version(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::string("0.3.0"))
}

/// Get package information
pub fn prim_package_info(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        list(vec![
            Value::string("Elle"),
            Value::string("0.3.0"),
            Value::string("A Lisp interpreter with bytecode compilation"),
        ]),
    )
}

/// Declarative primitive definitions for package operations
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "pkg/version",
        func: prim_package_version,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Get the current package version",
        params: &[],
        category: "pkg",
        example: "(pkg/version)",
        aliases: &["package-version"],
    },
    PrimitiveDef {
        name: "pkg/info",
        func: prim_package_info,
        effect: Effect::none(),
        arity: Arity::Exact(0),
        doc: "Get package information (name, version, description)",
        params: &[],
        category: "pkg",
        example: "(pkg/info)",
        aliases: &["package-info"],
    },
];
