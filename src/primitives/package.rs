use crate::epoch::CURRENT_EPOCH;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::{list, Value};

/// Get the current package version
pub(crate) fn prim_package_version(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::string(env!("CARGO_PKG_VERSION")))
}

/// Get the current language epoch
pub(crate) fn prim_epoch(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::int(CURRENT_EPOCH as i64))
}

/// Get package information
pub(crate) fn prim_package_info(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_OK,
        list(vec![
            Value::string("Elle"),
            Value::string(env!("CARGO_PKG_VERSION")),
            Value::string("A Lisp interpreter with bytecode compilation"),
        ]),
    )
}

/// Declarative primitive definitions for package operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "elle/version",
        func: prim_package_version,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Get the current package version",
        params: &[],
        category: "elle",
        example: "(elle/version)",
        aliases: &["pkg/version", "package-version"],
    },
    PrimitiveDef {
        name: "elle/epoch",
        func: prim_epoch,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return the current language epoch.",
        params: &[],
        category: "elle",
        example: "(elle/epoch) #=> 3",
        aliases: &[],
    },
    PrimitiveDef {
        name: "elle/info",
        func: prim_package_info,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Get package information (name, version, description)",
        params: &[],
        category: "elle",
        example: "(elle/info)",
        aliases: &["pkg/info", "package-info"],
    },
];
