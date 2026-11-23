// audited: 2026-11-23
// Unicode segmentation generations: which tables each VM selects, and who may choose.
//
// docs/strings.md
//
// The language suite (tests/lang/unicode.lisp) runs under the build's default
// generation. What needs another generation and a Rust type lives here: G16
// runtimes built through the embedding surface, a worker VM's generation, and
// the declaration's rules. tests/cli/unicode-flag.lisp holds the CLI flag and
// main-file selection.
//
// The divergence vector: U+10EFA is Extend in the Unicode 17 grapheme
// table and unassigned in Unicode 16 (the 16 table's range starts at
// U+10EFC), so "a\u{10EFA}" is one cluster under G17 and two under G16.

use crate::common::eval_source;
use elle::runtime::Runtime;
use elle::segment::Generation;
use elle::{eval_all, Value};

/// "a" followed by U+10EFA — the cross-generation divergence vector.
fn vector() -> String {
    format!("a{}", '\u{10EFA}')
}

/// Evaluate with an explicit generation, full stdlib.
fn eval_with_gen<R>(gen: Generation, input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    let mut rt = Runtime::with_unicode(gen);
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(input, symbols, vm, cctx, "<test>")
    };
    f(result)
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
    let src = format!("(os/join (os/spawn-vm (fn [] (length \"{}\"))))", vector());
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
    eval_with_gen(
        Generation::G16,
        "(unicode! 16) (if (= (unicode!) [16 0 0]) 1 0)",
        |r| {
            assert_eq!(r.expect("eval").as_int(), Some(1));
        },
    );
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
    // vm/config-set raises when it refuses a field, as it does for every
    // refusal (docs/config.md); the message explains the immutability.
    let src = "(let [[ok? err] (protect (vm/config-set :unicode [16 0 0]))] \
               (if (and (not ok?) \
                        (= (get err :error) :argument-error) \
                        (string/contains? (get err :message) \"fixed at VM construction\")) 1 0))";
    eval_source(src, |r| {
        assert_eq!(r.expect("eval").as_int(), Some(1));
    });
    // The set must not have taken effect.
    eval_source(
        "(protect (vm/config-set :unicode [16 0 0])) (if (= (vm/config :unicode) [17 0 0]) 1 0)",
        |r| {
            assert_eq!(r.expect("eval").as_int(), Some(1));
        },
    );
}
