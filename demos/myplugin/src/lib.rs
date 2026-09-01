//! The smallest plugin the stable ABI can express, and the one the literate
//! documentation loads.
//!
//! `docs/cookbook/plugins.md` walks through this crate line by line and then
//! imports it as `plugin/myplugin`; `docs/testing.md` gates its example on the
//! same import. `make doctest` runs both as programs, so this crate has to be
//! built for either document to reach the Elle below its import — the
//! Makefile's `myplugin` target builds it, and `tests/integration/doctest.rs`
//! pins the two together.
//!
//! Everything below the `use` is quoted verbatim in the cookbook: an edit here
//! is an edit to that document.

use elle_plugin::{ElleCtx, EllePrimDef, ElleResult, ElleValue, SIG_OK};

elle_plugin::define_plugin!("myplugin/", &PRIMITIVES);

extern "C" fn prim_hello(ctx: *mut ElleCtx, _args: *const ElleValue, nargs: usize) -> ElleResult {
    let a = api();
    if nargs != 0 {
        return a.err(ctx, "arity-error", "myplugin/hello: expected 0 arguments");
    }
    a.ok(a.string(ctx, "hello"))
}

static PRIMITIVES: &[EllePrimDef] = &[EllePrimDef::exact(
    "myplugin/hello",
    prim_hello,
    SIG_OK,
    0,
    "Say hello.",
    "myplugin",
    "(myplugin/hello)",
)];
