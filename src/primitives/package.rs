use crate::epoch::CURRENT_EPOCH;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::{list, Value};

/// Get the current package version
pub(crate) fn prim_package_version(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::string(env!("CARGO_PKG_VERSION")))
}

/// Get the current language epoch.
/// With 0 args: returns the current epoch.
/// With 1 arg: identity (the compiler strips `(elle/epoch N)` before this runs).
pub(crate) fn prim_epoch(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        (SIG_OK, Value::int(CURRENT_EPOCH as i64))
    } else {
        (SIG_OK, args[0])
    }
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

// Declarative primitive definitions for package operations
primitive! {
    "elle/version" => prim_package_version {
        doc: "Get the current package version",
        category: "elle",
        example: "(elle/version)",
        aliases: &["pkg/version", "package-version"],
    }
    "elle/epoch" => prim_epoch {
        arity: Arity::Range(0, 1),
        doc: "Return the current language epoch. With 1 arg, returns the arg (compile-time declaration form).",
        params: &["n"],
        category: "elle",
        example: "(elle/epoch) #=> 3",
    }
    "elle/info" => prim_package_info {
        doc: "Get package information (name, version, description)",
        category: "elle",
        example: "(elle/info)",
        aliases: &["pkg/info", "package-info"],
    }
}
