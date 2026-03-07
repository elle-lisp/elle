// Integration tests for the glob plugin (.so loaded via import-file).
// Most tests have been migrated to tests/elle/glob.lisp.
// This file retains only the plugin availability gate test.

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
