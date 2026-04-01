//! Elle random plugin — pseudo-random and cryptographically secure random
//! number generation via the `rand` and `rand_chacha` crates.

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};
use rand::seq::SliceRandom;
use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use std::f64::consts::PI;
use std::sync::{Mutex, OnceLock};

fn rng() -> &'static Mutex<StdRng> {
    static RNG: OnceLock<Mutex<StdRng>> = OnceLock::new();
    RNG.get_or_init(|| Mutex::new(StdRng::from_os_rng()))
}

fn csprng() -> &'static Mutex<ChaCha20Rng> {
    static CSPRNG: OnceLock<Mutex<ChaCha20Rng>> = OnceLock::new();
    CSPRNG.get_or_init(|| Mutex::new(ChaCha20Rng::from_os_rng()))
}

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
elle::elle_plugin_init!(PRIMITIVES, "random/");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract elements from an @array (mutable) or array (immutable) or list.
fn extract_elements(val: &Value) -> Option<Vec<Value>> {
    if let Some(arr) = val.as_array_mut() {
        return Some(arr.borrow().clone());
    }
    if let Some(tup) = val.as_array() {
        return Some(tup.to_vec());
    }
    if let Ok(v) = val.list_to_vec() {
        return Some(v);
    }
    None
}

/// Extract a float from a Value (float or int).
fn extract_float(val: &Value) -> Option<f64> {
    if let Some(f) = val.as_float() {
        return Some(f);
    }
    if let Some(i) = val.as_int() {
        return Some(i as f64);
    }
    None
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
    *rng().lock().unwrap() = StdRng::seed_from_u64(seed);
    (SIG_OK, Value::NIL)
}

fn prim_random_int(args: &[Value]) -> (SignalBits, Value) {
    let mut rng = rng().lock().unwrap();
    let val = match args.len() {
        0 => rng.random::<u64>() as i64,
        1 => {
            let max = match args[0].as_int() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("random/int: expected integer, got {}", args[0].type_name()),
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
            rng.random_range(0..max)
        }
        2 => {
            let min = match args[0].as_int() {
                Some(n) => n,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("random/int: expected integer, got {}", args[0].type_name()),
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
                            format!("random/int: expected integer, got {}", args[1].type_name()),
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
            rng.random_range(min..max)
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
}

fn prim_random_float(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::float(rng().lock().unwrap().random::<f64>()))
}

fn prim_random_bool(_args: &[Value]) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(rng().lock().unwrap().random::<bool>()))
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
    rng().lock().unwrap().fill(&mut buf[..]);
    (SIG_OK, Value::bytes(buf))
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
                        "random/shuffle: expected array or list, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    elements.shuffle(&mut *rng().lock().unwrap());
    (SIG_OK, Value::array_mut(elements))
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
                        "random/choice: expected array or list, got {}",
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
    let idx = rng().lock().unwrap().random_range(0..elements.len());
    (SIG_OK, elements[idx])
}

fn prim_random_normal(args: &[Value]) -> (SignalBits, Value) {
    let (mean, stddev) = match args.len() {
        0 => (0.0f64, 1.0f64),
        1 => {
            let m = match extract_float(&args[0]) {
                Some(f) => f,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "random/normal: expected number for mean, got {}",
                                args[0].type_name()
                            ),
                        ),
                    );
                }
            };
            (m, 1.0f64)
        }
        2 => {
            let m = match extract_float(&args[0]) {
                Some(f) => f,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "random/normal: expected number for mean, got {}",
                                args[0].type_name()
                            ),
                        ),
                    );
                }
            };
            let s = match extract_float(&args[1]) {
                Some(f) => f,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "random/normal: expected number for stddev, got {}",
                                args[1].type_name()
                            ),
                        ),
                    );
                }
            };
            (m, s)
        }
        _ => unreachable!("arity enforced by PRIMITIVES table"),
    };
    if stddev <= 0.0 {
        return (
            SIG_ERROR,
            error_val("range-error", "random/normal: stddev must be positive"),
        );
    }
    // Box-Muller transform
    let sample = {
        let mut r = rng().lock().unwrap();
        loop {
            let u1 = r.random::<f64>();
            let u2 = r.random::<f64>();
            if u1 > 0.0 {
                break mean + stddev * (-2.0 * u1.ln()).sqrt() * (2.0 * PI * u2).cos();
            }
        }
    };
    (SIG_OK, Value::float(sample))
}

fn prim_random_exponential(args: &[Value]) -> (SignalBits, Value) {
    let lambda = match args.len() {
        0 => 1.0f64,
        1 => match extract_float(&args[0]) {
            Some(f) => f,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "random/exponential: expected number for lambda, got {}",
                            args[0].type_name()
                        ),
                    ),
                );
            }
        },
        _ => unreachable!("arity enforced by PRIMITIVES table"),
    };
    if lambda <= 0.0 {
        return (
            SIG_ERROR,
            error_val("range-error", "random/exponential: lambda must be positive"),
        );
    }
    let u: f64 = rng().lock().unwrap().random::<f64>();
    let sample = -(1.0 - u).ln() / lambda;
    (SIG_OK, Value::float(sample))
}

fn prim_random_weighted(args: &[Value]) -> (SignalBits, Value) {
    let items = match extract_elements(&args[0]) {
        Some(elems) => elems,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/weighted: expected array or list for items, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let weight_vals = match extract_elements(&args[1]) {
        Some(elems) => elems,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/weighted: expected array or list for weights, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    if items.is_empty() {
        return (
            SIG_ERROR,
            error_val("range-error", "random/weighted: items must not be empty"),
        );
    }
    if items.len() != weight_vals.len() {
        return (
            SIG_ERROR,
            error_val(
                "range-error",
                format!(
                    "random/weighted: items and weights must have equal length, got {} and {}",
                    items.len(),
                    weight_vals.len()
                ),
            ),
        );
    }
    // Extract and validate weights
    let mut weights = Vec::with_capacity(weight_vals.len());
    for (i, wv) in weight_vals.iter().enumerate() {
        let w = match extract_float(wv) {
            Some(f) => f,
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "random/weighted: weight {} must be a number, got {}",
                            i,
                            wv.type_name()
                        ),
                    ),
                );
            }
        };
        if w <= 0.0 {
            return (
                SIG_ERROR,
                error_val(
                    "range-error",
                    format!("random/weighted: weight {} must be positive, got {}", i, w),
                ),
            );
        }
        weights.push(w);
    }
    // Prefix-sum cumulative distribution
    let mut cumsum = Vec::with_capacity(weights.len());
    let mut total = 0.0f64;
    for w in &weights {
        total += w;
        cumsum.push(total);
    }
    let pick = rng().lock().unwrap().random_range(0.0..total);
    let idx = cumsum.partition_point(|&c| c <= pick);
    let idx = idx.min(items.len() - 1);
    (SIG_OK, items[idx])
}

fn prim_random_csprng_bytes(args: &[Value]) -> (SignalBits, Value) {
    let len = match args[0].as_int() {
        Some(n) if n >= 0 => n as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val(
                    "range-error",
                    "random/csprng-bytes: length must be non-negative",
                ),
            );
        }
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/csprng-bytes: expected integer, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let mut buf = vec![0u8; len];
    csprng().lock().unwrap().fill(&mut buf[..]);
    (SIG_OK, Value::bytes(buf))
}

fn prim_random_csprng_seed(args: &[Value]) -> (SignalBits, Value) {
    // Extract bytes from bytes or @bytes value
    let data: Vec<u8> = if let Some(b) = args[0].as_bytes() {
        b.to_vec()
    } else if let Some(bm) = args[0].as_bytes_mut() {
        bm.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "random/csprng-seed: expected bytes, got {}",
                    args[0].type_name()
                ),
            ),
        );
    };
    if data.len() != 32 {
        return (
            SIG_ERROR,
            error_val(
                "range-error",
                format!(
                    "random/csprng-seed: expected exactly 32 bytes, got {}",
                    data.len()
                ),
            ),
        );
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&data);
    *csprng().lock().unwrap() = ChaCha20Rng::from_seed(seed);
    (SIG_OK, Value::NIL)
}

fn prim_random_sample(args: &[Value]) -> (SignalBits, Value) {
    let elements = match extract_elements(&args[0]) {
        Some(elems) => elems,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/sample: expected array or list, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let n = match args[1].as_int() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "random/sample: expected integer for n, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    if n < 0 || n as usize > elements.len() {
        return (
            SIG_ERROR,
            error_val(
                "range-error",
                format!(
                    "random/sample: n must be between 0 and {} (collection length), got {}",
                    elements.len(),
                    n
                ),
            ),
        );
    }
    let n = n as usize;
    // Partial Fisher-Yates: shuffle first n elements from a clone
    let mut pool = elements;
    {
        let mut r = rng().lock().unwrap();
        for i in 0..n {
            let j = r.random_range(i..pool.len());
            pool.swap(i, j);
        }
    }
    pool.truncate(n);
    (SIG_OK, Value::array(pool))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "random/seed",
        func: prim_random_seed,
        signal: Signal::errors(),
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
        signal: Signal::errors(),
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
        signal: Signal::silent(),
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
        signal: Signal::silent(),
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
        signal: Signal::errors(),
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
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return a new @array with elements shuffled randomly",
        params: &["collection"],
        category: "random",
        example: "(random/shuffle [1 2 3 4 5])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/choice",
        func: prim_random_choice,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return a random element from an array or list",
        params: &["collection"],
        category: "random",
        example: "(random/choice [1 2 3 4 5])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/normal",
        func: prim_random_normal,
        signal: Signal::errors(),
        arity: Arity::Range(0, 2),
        doc: "Sample from a normal distribution. 0 args: mean=0 stddev=1. 1 arg: mean=arg stddev=1. 2 args: mean and stddev.",
        params: &["[mean]", "[stddev]"],
        category: "random",
        example: "(random/normal 0.0 1.0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/exponential",
        func: prim_random_exponential,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Sample from an exponential distribution. 0 args: lambda=1. 1 arg: lambda=arg.",
        params: &["[lambda]"],
        category: "random",
        example: "(random/exponential 2.0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/weighted",
        func: prim_random_weighted,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Choose a random item from a collection according to corresponding weights",
        params: &["items", "weights"],
        category: "random",
        example: "(random/weighted [\"a\" \"b\" \"c\"] [1.0 2.0 3.0])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/csprng-bytes",
        func: prim_random_csprng_bytes,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Generate cryptographically secure random bytes of the given length",
        params: &["length"],
        category: "random",
        example: "(random/csprng-bytes 32)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/csprng-seed",
        func: prim_random_csprng_seed,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Seed the CSPRNG with exactly 32 bytes for deterministic output",
        params: &["seed-bytes"],
        category: "random",
        example: "(random/csprng-seed (bytes 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "random/sample",
        func: prim_random_sample,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return n randomly selected elements from a collection (no replacement)",
        params: &["collection", "n"],
        category: "random",
        example: "(random/sample [1 2 3 4 5] 3)",
        aliases: &[],
    },
];
