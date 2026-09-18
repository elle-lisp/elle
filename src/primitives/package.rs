// audited: 2026-09-17
//! What this build of Elle is: its version, its language epoch, and the cargo
//! profile it was compiled under.

use crate::epoch::CURRENT_EPOCH;
use crate::primitives::def::RegionEffect;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Get the current package version
pub(crate) fn prim_package_version(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, ctx.string(env!("CARGO_PKG_VERSION")))
}

/// Get the current language epoch.
/// With 0 args: returns the current epoch.
/// With 1 arg: identity (the compiler strips `(elle/epoch N)` before this runs).
pub(crate) fn prim_epoch(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() {
        (SIG_OK, Value::int(CURRENT_EPOCH as i64))
    } else {
        (SIG_OK, args[0])
    }
}

/// The cargo profile this binary was compiled under.
///
/// `debug_assertions` is the profile's own switch: cargo sets it for `dev`
/// and clears it for `release`, and module resolution already reads it to
/// pick `target/debug` over `target/release`. No other reading of the profile
/// survives into the running binary.
pub(crate) fn prim_build_profile(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    (SIG_OK, ctx.string(profile))
}

/// The path of the running binary, or nil when the OS will not say.
///
/// A program that wants to run Elle again has to name the Elle it is already
/// running, and nothing else in the process can: `(sys/argv)` carries the
/// source it was pointed at, and a subcommand replaces even that. Resolving
/// `elle` off `PATH` would answer with a different build, so the test runner
/// spawns this (docs/test-cli.md § Substrate).
pub(crate) fn prim_executable(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match std::env::current_exe() {
        Ok(path) => {
            let owned = path.to_string_lossy().into_owned();
            (SIG_OK, ctx.string(&owned))
        }
        Err(_) => (SIG_OK, Value::NIL),
    }
}

/// Get package information
pub(crate) fn prim_package_info(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let items = vec![
        ctx.string("Elle"),
        ctx.string(env!("CARGO_PKG_VERSION")),
        ctx.string("A Lisp interpreter with bytecode compilation"),
    ];
    (SIG_OK, ctx.list(items))
}

// Declarative primitive definitions for package operations
primitive! {
    "elle/version" => prim_package_version {
        doc: "Get the current package version",
        category: "elle",
        example: "(elle/version)",
        aliases: &["pkg/version", "package-version"],
        effect: RegionEffect::Fresh,
    }
    "elle/epoch" => prim_epoch {
        arity: Arity::Range(0, 1),
        doc: "Return the current language epoch. With 1 arg, returns the arg (compile-time declaration form).",
        params: &["n"],
        category: "elle",
        example: "(elle/epoch) #=> 3",
        effect: RegionEffect::PassThrough,
    }
    "elle/build-profile" => prim_build_profile {
        doc: "Return the cargo profile this binary was compiled under: \"debug\" or \"release\".",
        category: "elle",
        example: "(elle/build-profile)",
        effect: RegionEffect::Fresh,
    }
    "elle/executable" => prim_executable {
        doc: "Return the path of the running elle binary, or nil when the OS will not say.",
        category: "elle",
        example: "(elle/executable)",
        effect: RegionEffect::Fresh,
    }
    "elle/info" => prim_package_info {
        doc: "Get package information (name, version, description)",
        category: "elle",
        example: "(elle/info)",
        aliases: &["pkg/info", "package-info"],
        effect: RegionEffect::Fresh,
    }
}
