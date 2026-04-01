//! Elle semver plugin — semantic version parsing and comparison via the `semver` crate.

use std::collections::BTreeMap;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------
elle::elle_plugin_init!(PRIMITIVES, "semver/");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_string(val: &Value, name: &str, pos: &str) -> Result<String, (SignalBits, Value)> {
    val.with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be a string, got {}",
                    name,
                    pos,
                    val.type_name()
                ),
            ),
        )
    })
}

fn parse_version(s: &str, name: &str) -> Result<semver::Version, (SignalBits, Value)> {
    semver::Version::parse(s).map_err(|e| {
        (
            SIG_ERROR,
            error_val(
                "semver-error",
                format!("{}: invalid version {:?}: {}", name, s, e),
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_parse(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("semver/parse: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let ver_str = match get_string(&args[0], "semver/parse", "version") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let v = match parse_version(&ver_str, "semver/parse") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let mut fields = BTreeMap::new();
    fields.insert(
        TableKey::Keyword("major".into()),
        Value::int(v.major as i64),
    );
    fields.insert(
        TableKey::Keyword("minor".into()),
        Value::int(v.minor as i64),
    );
    fields.insert(
        TableKey::Keyword("patch".into()),
        Value::int(v.patch as i64),
    );
    fields.insert(
        TableKey::Keyword("pre".into()),
        Value::string(v.pre.to_string()),
    );
    fields.insert(
        TableKey::Keyword("build".into()),
        Value::string(v.build.to_string()),
    );
    (SIG_OK, Value::struct_from(fields))
}

fn prim_valid(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("semver/valid?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let ver_str = match get_string(&args[0], "semver/valid?", "version") {
        Ok(s) => s,
        Err(e) => return e,
    };
    (
        SIG_OK,
        Value::bool(semver::Version::parse(&ver_str).is_ok()),
    )
}

fn prim_compare(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("semver/compare: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let a_str = match get_string(&args[0], "semver/compare", "first version") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let b_str = match get_string(&args[1], "semver/compare", "second version") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let a = match parse_version(&a_str, "semver/compare") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let b = match parse_version(&b_str, "semver/compare") {
        Ok(v) => v,
        Err(e) => return e,
    };
    use std::cmp::Ordering;
    match a.cmp(&b) {
        Ordering::Less => (SIG_OK, Value::int(-1)),
        Ordering::Equal => (SIG_OK, Value::int(0)),
        Ordering::Greater => (SIG_OK, Value::int(1)),
    }
}

fn prim_satisfies(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "semver/satisfies?: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let ver_str = match get_string(&args[0], "semver/satisfies?", "version") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let req_str = match get_string(&args[1], "semver/satisfies?", "requirement") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let ver = match parse_version(&ver_str, "semver/satisfies?") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let req = match semver::VersionReq::parse(&req_str) {
        Ok(r) => r,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val(
                    "semver-error",
                    format!(
                        "semver/satisfies?: invalid requirement {:?}: {}",
                        req_str, e
                    ),
                ),
            )
        }
    };
    (SIG_OK, Value::bool(req.matches(&ver)))
}

fn prim_increment(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("semver/increment: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let ver_str = match get_string(&args[0], "semver/increment", "version") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut ver = match parse_version(&ver_str, "semver/increment") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let part = match args[1].as_keyword_name() {
        Some(k) => k,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                    "semver/increment: part must be a keyword (:major, :minor, or :patch), got {}",
                    args[1].type_name()
                ),
                ),
            )
        }
    };
    match part.as_str() {
        "major" => {
            ver.major += 1;
            ver.minor = 0;
            ver.patch = 0;
            ver.pre = semver::Prerelease::EMPTY;
            ver.build = semver::BuildMetadata::EMPTY;
        }
        "minor" => {
            ver.minor += 1;
            ver.patch = 0;
            ver.pre = semver::Prerelease::EMPTY;
            ver.build = semver::BuildMetadata::EMPTY;
        }
        "patch" => {
            ver.patch += 1;
            ver.pre = semver::Prerelease::EMPTY;
            ver.build = semver::BuildMetadata::EMPTY;
        }
        other => {
            return (
                SIG_ERROR,
                error_val(
                    "semver-error",
                    format!(
                        "semver/increment: unknown part {:?}, expected :major, :minor, or :patch",
                        other
                    ),
                ),
            )
        }
    }
    (SIG_OK, Value::string(ver.to_string()))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "semver/parse",
        func: prim_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a semver string. Returns {:major int :minor int :patch int :pre string :build string}.",
        params: &["version"],
        category: "semver",
        example: r#"(semver/parse "1.2.3")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "semver/valid?",
        func: prim_valid,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Check if a string is a valid semver version. Returns bool. Only errors on non-string input.",
        params: &["version"],
        category: "semver",
        example: r#"(semver/valid? "1.2.3")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "semver/compare",
        func: prim_compare,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compare two semver version strings. Returns -1, 0, or 1.",
        params: &["a", "b"],
        category: "semver",
        example: r#"(semver/compare "1.0.0" "2.0.0")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "semver/satisfies?",
        func: prim_satisfies,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if a version satisfies a requirement string (e.g., \">=1.0.0, <2.0.0\"). Returns bool.",
        params: &["version", "req"],
        category: "semver",
        example: r#"(semver/satisfies? "1.2.3" ">=1.0.0")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "semver/increment",
        func: prim_increment,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Increment a version part. Part is :major, :minor, or :patch. Clears lower parts and pre-release. Returns new version string.",
        params: &["version", "part"],
        category: "semver",
        example: r#"(semver/increment "1.2.3" :patch)"#,
        aliases: &[],
    },
];
