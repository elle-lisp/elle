//! Elle glob plugin — file globbing and pattern matching via the `glob` crate.

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use glob::Pattern;
use std::collections::BTreeMap;
use std::path::Path;

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("glob/").unwrap_or(def.name);
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

fn prim_glob_glob(args: &[Value]) -> (SignalBits, Value) {
    if let Some(result) = args[0].with_string(|pattern_str| {
        let mut results = Vec::new();
        match glob::glob(pattern_str) {
            Ok(paths) => {
                for entry in paths {
                    match entry {
                        Ok(path) => {
                            if let Some(path_str) = path.to_str() {
                                results.push(Value::string(path_str));
                            }
                        }
                        Err(_) => {
                            // Skip errored entries and continue collecting results
                        }
                    }
                }
                (SIG_OK, Value::array_mut(results))
            }
            Err(_) => (
                SIG_ERROR,
                error_val(
                    "pattern-error",
                    format!("glob/glob: invalid pattern: {}", pattern_str),
                ),
            ),
        }
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("glob/glob: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

fn prim_glob_match(args: &[Value]) -> (SignalBits, Value) {
    if let Some(result) = args[0].with_string(|pattern_str| {
        if let Some(result2) = args[1].with_string(|test_str| match Pattern::new(pattern_str) {
            Ok(pattern) => (SIG_OK, Value::bool(pattern.matches(test_str))),
            Err(_) => (
                SIG_ERROR,
                error_val(
                    "pattern-error",
                    format!("glob/match?: invalid pattern: {}", pattern_str),
                ),
            ),
        }) {
            result2
        } else {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("glob/match?: expected string, got {}", args[1].type_name()),
                ),
            )
        }
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("glob/match?: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

fn prim_glob_match_path(args: &[Value]) -> (SignalBits, Value) {
    if let Some(result) = args[0].with_string(|pattern_str| {
        if let Some(result2) = args[1].with_string(|path_str| match Pattern::new(pattern_str) {
            Ok(pattern) => (
                SIG_OK,
                Value::bool(pattern.matches_path(Path::new(path_str))),
            ),
            Err(_) => (
                SIG_ERROR,
                error_val(
                    "pattern-error",
                    format!("glob/match-path?: invalid pattern: {}", pattern_str),
                ),
            ),
        }) {
            result2
        } else {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "glob/match-path?: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "glob/match-path?: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "glob/glob",
        func: prim_glob_glob,
        signal: Signal::inert(),
        arity: Arity::Exact(1),
        doc: "Return array of file paths matching a glob pattern",
        params: &["pattern"],
        category: "glob",
        example: "(glob/glob \"src/**/*.rs\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "glob/match?",
        func: prim_glob_match,
        signal: Signal::inert(),
        arity: Arity::Exact(2),
        doc: "Test if a string matches a glob pattern",
        params: &["pattern", "str"],
        category: "glob",
        example: "(glob/match? \"*.rs\" \"main.rs\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "glob/match-path?",
        func: prim_glob_match_path,
        signal: Signal::inert(),
        arity: Arity::Exact(2),
        doc: "Test if a path matches a glob pattern (separator-aware)",
        params: &["pattern", "path"],
        category: "glob",
        example: "(glob/match-path? \"src/*.rs\" \"src/main.rs\")",
        aliases: &[],
    },
];
