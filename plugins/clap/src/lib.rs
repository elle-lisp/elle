//! Elle clap plugin — CLI argument parsing via the `clap` crate.

use clap::{Arg, ArgAction, ArgMatches, Command};
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
// Helpers
// ---------------------------------------------------------------------------

/// Extract a Vec<Value> from a Value that is a list, array, or @array.
/// Returns None if the value is none of these.
fn extract_vec(val: &Value) -> Option<Vec<Value>> {
    if let Some(arr_ref) = val.as_array_mut() {
        return Some(arr_ref.borrow().clone());
    }
    if let Some(arr) = val.as_array() {
        return Some(arr.to_vec());
    }
    if let Ok(v) = val.list_to_vec() {
        return Some(v);
    }
    None
}

/// Get an optional string field from an immutable struct map.
/// Returns None if the key is absent. Returns Err if the key is present
/// but its value is not a string.
fn get_opt_string(
    map: &BTreeMap<TableKey, Value>,
    key: &str,
    ctx: &str,
) -> Result<Option<String>, (SignalBits, Value)> {
    match map.get(&TableKey::Keyword(key.into())).copied() {
        None => Ok(None),
        Some(v) => match v.with_string(|s| s.to_string()) {
            Some(s) => Ok(Some(s)),
            None => Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: :{} must be a string, got {}", ctx, key, v.type_name()),
                ),
            )),
        },
    }
}

/// Get an optional bool field from an immutable struct map.
/// Returns false if absent, true if the value is truthy (we accept any bool Value).
fn get_opt_bool(map: &BTreeMap<TableKey, Value>, key: &str) -> bool {
    match map.get(&TableKey::Keyword(key.into())).copied() {
        None => false,
        Some(v) => v == Value::TRUE,
    }
}

/// Build a clap::Arg from an Elle arg spec struct.
///
/// Expected keys: :name (required string), :long, :short, :help, :action,
/// :required, :default, :value (all optional).
///
/// Returns Err((SignalBits, Value)) on any validation failure.
fn build_arg(
    arg_spec: &Value,
    has_commands: bool,
) -> Result<(String, Arg, String), (SignalBits, Value)> {
    let map = match arg_spec.as_struct() {
        Some(m) => m,
        None => {
            return Err((
                SIG_ERROR,
                error_val(
                    "clap-error",
                    "clap/parse: each arg must be a struct".to_string(),
                ),
            ));
        }
    };

    // :name — required
    let name = match map.get(&TableKey::Keyword("name".into())).copied() {
        None => {
            return Err((
                SIG_ERROR,
                error_val("clap-error", "clap/parse: each arg must have a :name key"),
            ));
        }
        Some(v) => match v.with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("clap/parse: :name must be a string, got {}", v.type_name()),
                    ),
                ));
            }
        },
    };

    // Check reserved names when :commands is present
    if has_commands && (name == "command" || name == "command-args") {
        return Err((
            SIG_ERROR,
            error_val(
                "clap-error",
                format!(
                    "clap/parse: arg name {} conflicts with reserved subcommand key",
                    name
                ),
            ),
        ));
    }

    // :action — optional keyword, default :set
    let action_str = match map.get(&TableKey::Keyword("action".into())).copied() {
        None => "set".to_string(),
        Some(v) => match v.as_keyword_name() {
            Some(s) => s,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "clap-error",
                        format!(
                            "clap/parse: :action must be a keyword, got {}",
                            v.type_name()
                        ),
                    ),
                ));
            }
        },
    };

    let clap_action = match action_str.as_str() {
        "set" => ArgAction::Set,
        "flag" => ArgAction::SetTrue,
        "count" => ArgAction::Count,
        "append" => ArgAction::Append,
        other => {
            return Err((
                SIG_ERROR,
                error_val(
                    "clap-error",
                    format!(
                        "clap/parse: unknown action :{}, expected :set, :flag, :count, or :append",
                        other
                    ),
                ),
            ));
        }
    };

    // Build the Arg
    // Pass name.clone() so `name` remains available for the return value.
    let mut arg = Arg::new(name.clone()).action(clap_action);

    // :long — optional string
    if let Some(long) = get_opt_string(map, "long", "clap/parse")? {
        arg = arg.long(long);
    }

    // :short — optional single-character string
    if let Some(short_val) = map.get(&TableKey::Keyword("short".into())).copied() {
        let short_s = match short_val.with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "clap-error",
                        format!(
                            "clap/parse: :short must be a single character, got {}",
                            short_val.type_name()
                        ),
                    ),
                ));
            }
        };
        let chars: Vec<char> = short_s.chars().collect();
        if chars.len() != 1 {
            return Err((
                SIG_ERROR,
                error_val(
                    "clap-error",
                    format!(
                        "clap/parse: :short must be a single character, got {:?}",
                        short_s
                    ),
                ),
            ));
        }
        arg = arg.short(chars[0]);
    }

    // :help — optional string
    if let Some(help) = get_opt_string(map, "help", "clap/parse")? {
        arg = arg.help(help);
    }

    // :required — optional bool
    if get_opt_bool(map, "required") {
        arg = arg.required(true);
    }

    // :default — optional string (only meaningful for :set action)
    if let Some(default) = get_opt_string(map, "default", "clap/parse")? {
        arg = arg.default_value(default);
    }

    // :value — optional string (metavar for help display)
    if let Some(value_name) = get_opt_string(map, "value", "clap/parse")? {
        arg = arg.value_name(value_name);
    }

    Ok((name, arg, action_str))
}

/// Build a clap::Command from an Elle command spec struct.
///
/// Expected top-level keys: :name (required), :about, :version, :args, :commands.
/// Calls itself recursively for each element in :commands.
///
/// Returns Err((SignalBits, Value)) on any validation failure.
fn build_command(spec: &Value) -> Result<Command, (SignalBits, Value)> {
    let map = match spec.as_struct() {
        Some(m) => m,
        None => {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "clap/parse: spec must be a struct, got {}",
                        spec.type_name()
                    ),
                ),
            ));
        }
    };

    // :name — required string
    let name = match map.get(&TableKey::Keyword("name".into())).copied() {
        None => {
            return Err((
                SIG_ERROR,
                error_val("clap-error", "clap/parse: spec must have a :name key"),
            ));
        }
        Some(v) => match v.with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("clap/parse: :name must be a string, got {}", v.type_name()),
                    ),
                ));
            }
        },
    };

    let mut cmd = Command::new(name)
        .disable_help_flag(false)
        .disable_version_flag(false);

    // :about — optional string
    if let Some(about) = get_opt_string(map, "about", "clap/parse")? {
        cmd = cmd.about(about);
    }

    // :version — optional string
    if let Some(version) = get_opt_string(map, "version", "clap/parse")? {
        cmd = cmd.version(version);
    }

    // Determine whether :commands is present (non-empty), so we can validate
    // reserved arg names before building args.
    let commands_vec: Vec<Value> = match map.get(&TableKey::Keyword("commands".into())).copied() {
        None => Vec::new(),
        Some(v) => match extract_vec(&v) {
            Some(elems) => elems,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "clap/parse: :commands must be an array or list, got {}",
                            v.type_name()
                        ),
                    ),
                ));
            }
        },
    };
    let has_commands = !commands_vec.is_empty();

    // :args — optional array/list of arg specs
    let args_vec: Vec<Value> = match map.get(&TableKey::Keyword("args".into())).copied() {
        None => Vec::new(),
        Some(v) => match extract_vec(&v) {
            Some(elems) => elems,
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "clap/parse: :args must be an array or list, got {}",
                            v.type_name()
                        ),
                    ),
                ));
            }
        },
    };

    for arg_spec in &args_vec {
        let (_, arg, _) = build_arg(arg_spec, has_commands)?;
        cmd = cmd.arg(arg);
    }

    // :commands — recurse
    for subcmd_spec in &commands_vec {
        let subcmd = build_command(subcmd_spec)?;
        cmd = cmd.subcommand(subcmd);
    }

    Ok(cmd)
}

/// Convert ArgMatches to an Elle struct, using the spec to know each arg's action.
///
/// The spec is needed because ArgMatches does not expose what action was configured.
/// We walk spec's :args in parallel with ArgMatches extraction.
fn matches_to_value(matches: &ArgMatches, spec: &Value) -> Result<Value, (SignalBits, Value)> {
    let map = spec.as_struct().expect("spec must be a struct here");

    let args_vec: Vec<Value> = match map.get(&TableKey::Keyword("args".into())).copied() {
        None => Vec::new(),
        Some(v) => extract_vec(&v).unwrap_or_default(),
    };

    let commands_vec: Vec<Value> = match map.get(&TableKey::Keyword("commands".into())).copied() {
        None => Vec::new(),
        Some(v) => extract_vec(&v).unwrap_or_default(),
    };
    let has_commands = !commands_vec.is_empty();

    let mut result: BTreeMap<TableKey, Value> = BTreeMap::new();

    for arg_spec in &args_vec {
        // Re-parse the arg spec minimally: we only need :name and :action.
        let arg_map = arg_spec
            .as_struct()
            .expect("arg spec must be a struct here");

        let name = arg_map
            .get(&TableKey::Keyword("name".into()))
            .and_then(|v| v.with_string(|s| s.to_string()))
            .expect("arg :name was validated earlier");

        let action_str = match arg_map.get(&TableKey::Keyword("action".into())).copied() {
            None => "set".to_string(),
            Some(v) => v.as_keyword_name().unwrap_or_else(|| "set".to_string()),
        };

        let value = match action_str.as_str() {
            "set" => {
                // get_one::<String> returns Option<&String>
                match matches.get_one::<String>(name.as_str()) {
                    Some(s) => Value::string(s.as_str()),
                    None => Value::NIL,
                }
            }
            "flag" => {
                // get_flag returns bool
                Value::bool(matches.get_flag(name.as_str()))
            }
            "count" => {
                // get_count returns u8
                Value::int(matches.get_count(name.as_str()) as i64)
            }
            "append" => {
                // get_many::<String> returns Option<ValuesRef<String>>
                let strings: Vec<Value> = match matches.get_many::<String>(name.as_str()) {
                    Some(vals) => vals.map(|s| Value::string(s.as_str())).collect(),
                    None => Vec::new(),
                };
                Value::array(strings)
            }
            _ => Value::NIL, // unreachable: validated in build_arg
        };

        result.insert(TableKey::Keyword(name), value);
    }

    // Subcommand handling
    if has_commands {
        match matches.subcommand() {
            Some((subcmd_name, subcmd_matches)) => {
                // Find the matching subcommand spec to recurse
                let subcmd_spec = commands_vec.iter().find(|spec| {
                    spec.as_struct()
                        .and_then(|m| m.get(&TableKey::Keyword("name".into())).copied())
                        .and_then(|v| v.with_string(|s| s.to_string()))
                        .as_deref()
                        == Some(subcmd_name)
                });

                result.insert(
                    TableKey::Keyword("command".into()),
                    Value::string(subcmd_name),
                );

                match subcmd_spec {
                    Some(spec) => {
                        let sub_val = matches_to_value(subcmd_matches, spec)?;
                        result.insert(TableKey::Keyword("command-args".into()), sub_val);
                    }
                    None => {
                        // Should not happen if build_command succeeded, but be safe.
                        result.insert(TableKey::Keyword("command-args".into()), Value::NIL);
                    }
                }
            }
            None => {
                result.insert(TableKey::Keyword("command".into()), Value::NIL);
                result.insert(TableKey::Keyword("command-args".into()), Value::NIL);
            }
        }
    }

    Ok(Value::struct_from(result))
}

// ---------------------------------------------------------------------------
// Primitive
// ---------------------------------------------------------------------------

fn prim_clap_parse(args: &[Value]) -> (SignalBits, Value) {
    // args[0] = spec, args[1] = argv
    // Arity is Exact(2), so the VM guarantees args.len() == 2.

    // Validate spec: must be an immutable struct.
    if !args[0].is_struct() {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "clap/parse: spec must be a struct, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }

    // Validate argv: must be a list, array, or @array.
    let argv_elements = match extract_vec(&args[1]) {
        Some(v) => v,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "clap/parse: argv must be a list or array, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };

    // Validate argv elements: each must be a string.
    let mut argv_strings: Vec<String> = Vec::with_capacity(argv_elements.len());
    for elem in &argv_elements {
        match elem.with_string(|s| s.to_string()) {
            Some(s) => argv_strings.push(s),
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "clap/parse: argv element must be a string, got {}",
                            elem.type_name()
                        ),
                    ),
                );
            }
        }
    }

    // Build the clap::Command from the spec.
    let cmd = match build_command(&args[0]) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Run the parser.
    // disable_help_flag / disable_version_flag are NOT set to true here —
    // clap generates help and version automatically. However, --help and
    // --version cause clap to print and exit by default, which would call
    // std::process::exit in our process. We prevent this by using
    // try_get_matches_from which returns an error for --help/--version
    // instead of calling exit. The caller gets the formatted help text
    // in the error message.
    let result = cmd.try_get_matches_from(argv_strings);

    let matches = match result {
        Ok(m) => m,
        Err(e) => {
            return (SIG_ERROR, error_val("clap-error", e.to_string()));
        }
    };

    // Convert ArgMatches to an Elle struct.
    match matches_to_value(&matches, &args[0]) {
        Ok(v) => (SIG_OK, v),
        Err(e) => e,
    }
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
