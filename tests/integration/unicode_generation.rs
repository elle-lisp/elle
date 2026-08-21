// Unicode segmentation generations: per-VM table selection.
//
// The corpus (tests/elle/unicode.lisp) covers the default generation only,
// because corpus files compile on the shared runner VM. Everything that
// needs a non-default generation lives here: G16 runtimes built through
// the embedding surface, the CLI flag, main-file selection, and the
// program-wide agreement rules.
//
// The divergence vector: U+10EFA is Extend in the Unicode 17 grapheme
// table and unassigned in Unicode 16 (the 16 table's range starts at
// U+10EFC), so "a\u{10EFA}" is one cluster under G17 and two under G16.

use crate::common::eval_source;
use elle::runtime::Runtime;
use elle::segment::Generation;
use elle::{eval_all, Value};
use std::process::Command;

/// "a" followed by U+10EFA — the cross-generation divergence vector.
fn vector() -> String {
    format!("a{}", '\u{10EFA}')
}

/// Evaluate with an explicit generation, full stdlib.
fn eval_with_gen<R>(
    gen: Generation,
    input: &str,
    f: impl FnOnce(Result<Value, String>) -> R,
) -> R {
    let mut rt = Runtime::with_unicode(gen);
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(input, symbols, vm, cctx, "<test>")
    };
    f(result)
}

fn elle() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn run_cli(args: &[&str], source: &str) -> (String, String, std::process::ExitStatus) {
    let mut cmd = Command::new(elle());
    for a in args {
        cmd.arg(a);
    }
    cmd.arg("-e").arg(source);
    let out = cmd.output().expect("spawn elle");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status,
    )
}

// ── Embedding surface ────────────────────────────────────────────────────────

#[test]
fn g16_runtime_counts_two_clusters() {
    let src = format!("(length \"{}\")", vector());
    eval_with_gen(Generation::G16, &src, |r| {
        let v = r.expect("eval under G16");
        assert_eq!(v.as_int(), Some(2), "U+10EFA is unassigned in Unicode 16");
    });
}

#[test]
fn default_runtime_counts_one_cluster() {
    let src = format!("(length \"{}\")", vector());
    eval_source(&src, |r| {
        let v = r.expect("eval under default generation");
        assert_eq!(v.as_int(), Some(1), "U+10EFA is Extend in Unicode 17");
    });
}

#[test]
fn g16_get_returns_base_without_mark() {
    let src = format!("(if (= (get \"{}\" 0) \"a\") 1 0)", vector());
    eval_with_gen(Generation::G16, &src, |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
}

#[test]
fn worker_vm_inherits_generation() {
    let src = format!(
        "(os/join (os/spawn-vm (fn [] (length \"{}\"))))",
        vector()
    );
    eval_with_gen(Generation::G16, &src, |r| {
        let v = r.expect("spawned worker eval");
        assert_eq!(v.as_int(), Some(2), "worker VM must inherit G16");
    });
}

// ── unicode! declaration semantics ───────────────────────────────────────────

#[test]
fn unicode_zero_arg_folds_to_version() {
    eval_source("(if (= (unicode!) [17 0 0]) 1 0)", |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
}

#[test]
fn unicode_declaration_conflict_is_compile_error() {
    eval_source("(unicode! 16) 1", |r| {
        let err = r.expect_err("G16 declaration under a G17 lock must not compile");
        assert!(
            err.contains("Unicode 16") && err.contains("17.0.0"),
            "error must name both versions, got: {}",
            err
        );
    });
}

#[test]
fn unicode_not_vendored_is_compile_error() {
    eval_source("(unicode! 15)", |r| {
        let err = r.expect_err("unvendored generation must not compile");
        assert!(
            err.contains("not available in this build"),
            "error must say the generation is not vendored, got: {}",
            err
        );
        assert!(
            err.contains("16") && err.contains("17"),
            "error must list the vendored generations, got: {}",
            err
        );
    });
}

#[test]
fn unicode_component_must_be_integer_literal() {
    eval_source("(unicode! \"17\")", |r| {
        assert!(r.is_err(), "string component must be a compile error");
    });
    eval_source("(unicode! -1)", |r| {
        assert!(r.is_err(), "negative component must be a compile error");
    });
    eval_source("(unicode! 17 0 0 0)", |r| {
        assert!(r.is_err(), "four components must be an arity error");
    });
}

#[test]
fn matching_declaration_selects_under_g16_runtime() {
    eval_with_gen(Generation::G16, "(unicode! 16) (if (= (unicode!) [16 0 0]) 1 0)", |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
}

// ── vm/config introspection ──────────────────────────────────────────────────

#[test]
fn vm_config_unicode_is_readable() {
    eval_source("(if (= (vm/config :unicode) [17 0 0]) 1 0)", |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
    let src = "(if (= (vm/config :unicode) [16 0 0]) 1 0)";
    eval_with_gen(Generation::G16, src, |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
}

#[test]
fn vm_config_unicode_is_immutable() {
    // vm/config-set reports rejection as an error value (its contract for
    // every field); the message must explain the immutability.
    let src = "(def r (vm/config-set :unicode [16 0 0])) \
               (if (and (= (get r :error) :argument-error) \
                        (string/contains? (get r :message) \"fixed at VM construction\")) 1 0)";
    eval_source(src, |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
    // The set must not have taken effect.
    eval_source(
        "(vm/config-set :unicode [16 0 0]) (if (= (vm/config :unicode) [17 0 0]) 1 0)",
        |r| {
            assert_eq!(r.expect("eval").as_int(), Some(1));
        },
    );
}

// ── CLI surface ──────────────────────────────────────────────────────────────

#[test]
fn cli_unicode_16_selects_old_tables() {
    let src = format!("(println (length \"{}\"))", vector());
    let (out, err, status) = run_cli(&["--unicode=16.0"], &src);
    assert!(status.success(), "stderr: {}", err);
    assert!(out.contains('2'), "expected 2 clusters, got: {}", out);
}

#[test]
fn cli_default_uses_newest_tables() {
    let src = format!("(println (length \"{}\"))", vector());
    let (out, err, status) = run_cli(&[], &src);
    assert!(status.success(), "stderr: {}", err);
    assert!(out.contains('1'), "expected 1 cluster, got: {}", out);
}

#[test]
fn cli_flag_conflicting_with_declaration_fails() {
    let (_, err, status) = run_cli(&["--unicode=16"], "(unicode! 17)");
    assert!(!status.success(), "conflict must be fatal");
    assert!(
        err.contains("16") && err.contains("17"),
        "error must name both versions, got: {}",
        err
    );
}

#[test]
fn cli_unvendored_generation_fails() {
    let (_, err, status) = run_cli(&["--unicode=15"], "1");
    assert!(!status.success());
    assert!(
        err.contains("not available in this build"),
        "got: {}",
        err
    );
}

#[test]
fn cli_malformed_unicode_flag_fails() {
    let (_, err, status) = run_cli(&["--unicode=latest"], "1");
    assert!(!status.success());
    assert!(err.contains("--unicode"), "got: {}", err);
}

#[test]
fn main_file_declaration_selects_generation() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/unicode16.lisp");
    let out = Command::new(elle())
        .arg(fixture)
        .output()
        .expect("spawn elle");
    assert!(
        out.status.success(),
        "fixture must select G16 from its declaration:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
