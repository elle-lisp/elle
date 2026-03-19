//! Elle clap plugin — CLI argument parsing via the `clap` crate.

#[allow(unused_imports)]
use clap::{Arg, ArgAction, Command};
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
#[no_mangle]
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    // Route keyword operations to the host's global name table.
    // Must be called before any keyword is created or looked up.
    // We read keyword keys from Elle structs (:name, :args, :action, etc.)
    // so the host's keyword table must be used.
    ctx.init_keywords();

    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("clap/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_clap_parse(_args: &[Value]) -> (SignalBits, Value) {
    // Placeholder — full implementation in Chunk 2.
    (SIG_OK, Value::NIL)
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[PrimitiveDef {
    name: "clap/parse",
    func: prim_clap_parse,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Parse CLI arguments against a command spec. Returns a struct of parsed values.",
    params: &["spec", "argv"],
    category: "clap",
    example: r#"(clap/parse {:name "app" :args [{:name "verbose" :long "verbose" :action :flag}]} ["--verbose"])"#,
    aliases: &[],
}];
