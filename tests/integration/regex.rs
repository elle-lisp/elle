// Integration tests for the regex plugin (.so loaded via import-file).

use crate::common::eval_source;
use elle::Value;

/// Path to the compiled regex plugin shared object.
fn plugin_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/target/debug/libelle_regex.so", manifest)
}

/// Returns true if the plugin .so exists on disk.
fn plugin_available() -> bool {
    std::path::Path::new(&plugin_path()).exists()
}

// ── Availability gate ──────────────────────────────────────────────

#[test]
fn test_regex_plugin_loads() {
    if !plugin_available() {
        eprintln!("SKIP: regex plugin not built (run `cargo build -p elle-regex`)");
        return;
    }
    let result = eval_source(&format!(r#"(import-file "{}") :ok"#, plugin_path()));
    assert_eq!(result.unwrap(), Value::keyword("ok"));
}

// ── regex/compile ──────────────────────────────────────────────────

#[test]
fn test_regex_compile_valid() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/compile "\\d+")"#,
        plugin_path()
    ));
    assert!(result.is_ok(), "valid pattern should compile: {:?}", result);
}

#[test]
fn test_regex_compile_invalid_pattern() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/compile "[invalid")"#,
        plugin_path()
    ));
    assert!(result.is_err(), "invalid pattern should error");
}

#[test]
fn test_regex_compile_wrong_type() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/compile 42)"#,
        plugin_path()
    ));
    assert!(result.is_err(), "non-string should error");
}

#[test]
fn test_regex_compile_wrong_arity() {
    if !plugin_available() {
        return;
    }
    let p = plugin_path();
    assert!(eval_source(&format!(r#"(import-file "{}") (regex/compile)"#, p)).is_err());
    assert!(eval_source(&format!(r#"(import-file "{}") (regex/compile "a" "b")"#, p)).is_err());
}

// ── regex/match? ───────────────────────────────────────────────────

#[test]
fn test_regex_match_true() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/match? (regex/compile "\\d+") "abc123")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_regex_match_false() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/match? (regex/compile "\\d+") "abc")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::FALSE);
}

#[test]
fn test_regex_match_wrong_type() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/match? "not-a-regex" "abc")"#,
        plugin_path()
    ));
    assert!(result.is_err());
}

// ── regex/find ─────────────────────────────────────────────────────

#[test]
fn test_regex_find_match() {
    if !plugin_available() {
        return;
    }
    // Returns a struct {:match "123" :start 3 :end 6}
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def m (regex/find (regex/compile "\\d+") "abc123def"))
           (get m :match)"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::string("123"));
}

#[test]
fn test_regex_find_start_end() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def m (regex/find (regex/compile "\\d+") "abc123def"))
           @[(get m :start) (get m :end)]"#,
        plugin_path()
    ));
    let v = result.unwrap();
    let arr = v.as_array().unwrap();
    let arr = arr.borrow();
    assert_eq!(arr[0].as_int(), Some(3));
    assert_eq!(arr[1].as_int(), Some(6));
}

#[test]
fn test_regex_find_no_match() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/find (regex/compile "\\d+") "abc")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::NIL);
}

// ── regex/find-all ─────────────────────────────────────────────────

#[test]
fn test_regex_find_all_multiple() {
    if !plugin_available() {
        return;
    }
    // Returns a list of match structs
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def matches (regex/find-all (regex/compile "\\d+") "a1b22c333"))
           (length matches)"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::int(3));
}

#[test]
fn test_regex_find_all_values() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def matches (regex/find-all (regex/compile "\\d+") "a1b22c333"))
           (get (first matches) :match)"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::string("1"));
}

#[test]
fn test_regex_find_all_no_matches() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def matches (regex/find-all (regex/compile "\\d+") "abc"))
           (empty? matches)"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

// ── regex/captures ─────────────────────────────────────────────────

#[test]
fn test_regex_captures_numbered() {
    if !plugin_available() {
        return;
    }
    // Group 0 = full match, group 1 = first capture
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def c (regex/captures (regex/compile "(\\d+)-(\\w+)") "42-hello"))
           @[(get c :0) (get c :1) (get c :2)]"#,
        plugin_path()
    ));
    let v = result.unwrap();
    let arr = v.as_array().unwrap();
    let arr = arr.borrow();
    assert_eq!(arr[0], Value::string("42-hello")); // group 0: full match
    assert_eq!(arr[1], Value::string("42")); // group 1
    assert_eq!(arr[2], Value::string("hello")); // group 2
}

#[test]
fn test_regex_captures_named() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def c (regex/captures
                      (regex/compile "(?P<year>\\d{{4}})-(?P<month>\\d{{2}})")
                      "2024-01-15"))
           @[(get c :year) (get c :month)]"#,
        plugin_path()
    ));
    let v = result.unwrap();
    let arr = v.as_array().unwrap();
    let arr = arr.borrow();
    assert_eq!(arr[0], Value::string("2024"));
    assert_eq!(arr[1], Value::string("01"));
}

#[test]
fn test_regex_captures_no_match() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (regex/captures (regex/compile "\\d+") "abc")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::NIL);
}

// ── Error propagation ──────────────────────────────────────────────

#[test]
fn test_regex_find_wrong_arity() {
    if !plugin_available() {
        return;
    }
    let p = plugin_path();
    assert!(eval_source(&format!(
        r#"(import-file "{}") (regex/find (regex/compile "x"))"#,
        p
    ))
    .is_err());
}

#[test]
fn test_regex_captures_wrong_arity() {
    if !plugin_available() {
        return;
    }
    let p = plugin_path();
    assert!(eval_source(&format!(
        r#"(import-file "{}") (regex/captures (regex/compile "x"))"#,
        p
    ))
    .is_err());
}
