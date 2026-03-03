// Integration tests for the glob plugin (.so loaded via import-file).

use crate::common::eval_source;
use elle::Value;

/// Path to the compiled glob plugin shared object.
fn plugin_path() -> String {
    let manifest = env!("CARGO_MANIFEST_DIR");
    format!("{}/target/debug/libelle_glob.so", manifest)
}

/// Returns true if the plugin .so exists on disk.
fn plugin_available() -> bool {
    std::path::Path::new(&plugin_path()).exists()
}

// ── Availability gate ──────────────────────────────────────────────

#[test]
fn test_glob_plugin_loads() {
    if !plugin_available() {
        eprintln!("SKIP: glob plugin not built (run `cargo build -p elle-glob`)");
        return;
    }
    let result = eval_source(&format!(r#"(import-file "{}") :ok"#, plugin_path()));
    assert_eq!(result.unwrap(), Value::keyword("ok"));
}

// ── glob/glob ──────────────────────────────────────────────────────

#[test]
fn test_glob_glob_returns_array() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (array? (glob/glob "Cargo.toml"))"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_glob_glob_finds_cargo_toml() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def matches (glob/glob "Cargo.toml"))
           @[(length matches) (get matches 0)]"#,
        plugin_path()
    ));
    let v = result.unwrap();
    let arr = v.as_array().unwrap();
    let arr = arr.borrow();
    assert_eq!(arr[0].as_int(), Some(1));
    assert_eq!(arr[1], Value::string("Cargo.toml"));
}

#[test]
fn test_glob_glob_wildcard() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def matches (glob/glob "plugins/*/Cargo.toml"))
           (> (length matches) 0)"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_glob_glob_no_matches() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}")
           (def matches (glob/glob "nonexistent_*.xyz"))
           (= (length matches) 0)"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_glob_glob_invalid_pattern() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/glob "[invalid")"#,
        plugin_path()
    ));
    assert!(result.is_err(), "invalid pattern should error");
}

#[test]
fn test_glob_glob_wrong_type() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/glob 42)"#,
        plugin_path()
    ));
    assert!(result.is_err(), "non-string should error");
}

// ── glob/match? ────────────────────────────────────────────────────

#[test]
fn test_glob_match_true() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match? "*.rs" "main.rs")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_glob_match_false() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match? "*.rs" "main.py")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::FALSE);
}

#[test]
fn test_glob_match_invalid_pattern() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match? "[invalid" "test")"#,
        plugin_path()
    ));
    assert!(result.is_err(), "invalid pattern should error");
}

#[test]
fn test_glob_match_wrong_type() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match? 42 "test")"#,
        plugin_path()
    ));
    assert!(result.is_err(), "non-string should error");
}

// ── glob/match-path? ───────────────────────────────────────────────

#[test]
fn test_glob_match_path_true() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match-path? "src/*.rs" "src/main.rs")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::TRUE);
}

#[test]
fn test_glob_match_path_false() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match-path? "*.py" "src/main.rs")"#,
        plugin_path()
    ));
    assert_eq!(result.unwrap(), Value::FALSE);
}

#[test]
fn test_glob_match_path_invalid_pattern() {
    if !plugin_available() {
        return;
    }
    let result = eval_source(&format!(
        r#"(import-file "{}") (glob/match-path? "[invalid" "test")"#,
        plugin_path()
    ));
    assert!(result.is_err(), "invalid pattern should error");
}
