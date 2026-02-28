//! Elle random plugin — fast pseudo-random number generation via the `fastrand` crate.

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    static RNG: RefCell<fastrand::Rng> = RefCell::new(fastrand::Rng::new());
}

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
        let short_name = def.name.strip_prefix("random/").unwrap_or(def.name);
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

fn prim_random_seed(args: &[Value]) -> (SignalBits, Value) {
    let seed = match args[0].as_int() {
        Some(n) => n as u64,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("random/seed: expected integer, got {}", args[0].type_name()),
                ),
            );
        }
    };
    RNG.with(|rng| {
        *rng.borrow_mut() = fastrand::Rng::with_seed(seed);
    });
    (SIG_OK, Value::NIL)
}

fn prim_random_int(args: &[Value]) -> (SignalBits, Value) {
    RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        let val = match args.len() {
            0 => rng.u64(..) as i64,
            1 => {
                let max = match args[0].as_int() {
                    Some(n) => n,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!(
                                    "random/int: expected integer, got {}",
                                    args[0].type_name()
                                ),
                            ),
                        );
                    }
                };
                if max <= 0 {
                    return (
                        SIG_ERROR,
                        error_val("range-error", "random/int: max must be positive"),
                    );
                }
                rng.i64(0..max)
            }
            2 => {
                let min = match args[0].as_int() {
                    Some(n) => n,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!(
                                    "random/int: expected integer, got {}",
                                    args[0].type_name()
                                ),
                            ),
                        );
                    }
                };
                let max = match args[1].as_int() {
                    Some(n) => n,
                    None => {
                        return (
                            SIG_ERROR,
                            error_val(
                                "type-error",
                                format!(
                                    "random/int: expected integer, got {}",
                                    args[1].type_name()
                                ),
                            ),
                        );
                    }
                };
                if min >= max {
                    return (
                        SIG_ERROR,
                        error_val("range-error", "random/int: min must be less than max"),
                    );
                }
                rng.i64(min..max)
            }
            _ => {
                return (
                    SIG_ERROR,
                    error_val(
                        "arity-error",
                        format!("random/int: expected 0-2 arguments, got {}", args.len()),
                    ),
                );
            }
        };
        (SIG_OK, Value::int(val))
    })
}

fn prim_random_float(_args: &[Value]) -> (SignalBits, Value) {
    RNG.with(|rng| (SIG_OK, Value::float(rng.borrow_mut().f64())))
}

fn prim_random_bool(_args: &[Value]) -> (SignalBits, Value) {
    RNG.with(|rng| (SIG_OK, Value::bool(rng.borrow_mut().bool())))
}

fn prim_random_bytes(args: &[Value]) -> (SignalBits, Value) {
    let len = match args[0].as_int() {
        Some(n) if n >= 0 => n as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val("range-error", "random/bytes: length must be non-negative"),
            );
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/bytes: expected integer, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let mut buf = vec![0u8; len];
    RNG.with(|rng| {
        rng.borrow_mut().fill(&mut buf);
    });
    (SIG_OK, Value::bytes(buf))
}

/// Extract elements from an array (mutable) or tuple (immutable).
fn extract_elements(val: &Value) -> Option<Vec<Value>> {
    if let Some(arr) = val.as_array() {
        return Some(arr.borrow().clone());
    }
    if let Some(tup) = val.as_tuple() {
        return Some(tup.to_vec());
    }
    None
}

fn prim_random_shuffle(args: &[Value]) -> (SignalBits, Value) {
    let mut elements = match extract_elements(&args[0]) {
        Some(elems) => elems,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/shuffle: expected array or tuple, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    RNG.with(|rng| {
        rng.borrow_mut().shuffle(&mut elements);
    });
    (SIG_OK, Value::array(elements))
}

fn prim_random_choice(args: &[Value]) -> (SignalBits, Value) {
    let elements = match extract_elements(&args[0]) {
        Some(elems) => elems,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/choice: expected array or tuple, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    if elements.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "range-error",
                "random/choice: cannot choose from empty collection",
            ),
        );
    }
    RNG.with(|rng| {
        let idx = rng.borrow_mut().usize(0..elements.len());
        (SIG_OK, elements[idx])
    })
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "random/seed",
        func: prim_random_seed,
        effect: Effect::pure(),
        arity: Arity::Exact(1),
        doc: "Seed the PRNG for deterministic output",
        params: &["seed"],
        category: "random",
        example: "(random/seed 42)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/int",
        func: prim_random_int,
        effect: Effect::pure(),
        arity: Arity::Range(0, 2),
        doc: "Random integer. No args: full range. One arg: 0..n. Two args: min..max.",
        params: &["[max-or-min]", "[max]"],
        category: "random",
        example: "(random/int 100)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/float",
        func: prim_random_float,
        effect: Effect::pure(),
        arity: Arity::Exact(0),
        doc: "Random float in [0, 1)",
        params: &[],
        category: "random",
        example: "(random/float)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/bool",
        func: prim_random_bool,
        effect: Effect::pure(),
        arity: Arity::Exact(0),
        doc: "Random boolean",
        params: &[],
        category: "random",
        example: "(random/bool)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/bytes",
        func: prim_random_bytes,
        effect: Effect::pure(),
        arity: Arity::Exact(1),
        doc: "Generate a byte vector of the given length filled with random bytes",
        params: &["length"],
        category: "random",
        example: "(random/bytes 16)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/shuffle",
        func: prim_random_shuffle,
        effect: Effect::pure(),
        arity: Arity::Exact(1),
        doc: "Return a new array with elements shuffled randomly",
        params: &["collection"],
        category: "random",
        example: "(random/shuffle @[1 2 3 4 5])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/choice",
        func: prim_random_choice,
        effect: Effect::pure(),
        arity: Arity::Exact(1),
        doc: "Return a random element from an array or tuple",
        params: &["collection"],
        category: "random",
        example: "(random/choice @[1 2 3 4 5])",
        aliases: &[],
    },
];
