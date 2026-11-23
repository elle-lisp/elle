// audited: 2026-11-22
//! Pins how the JIT verifier setting is read: the build's default, the flag
//! that overrides it, and the values the flag refuses.
//!
//! docs/config.md
//! docs/impl/verifier.md

use super::*;

/// A build with debug assertions verifies by default, and every profile the
/// Rust tests build carries them. The counter-factual: a default of off, which
/// would run every Rust test that drives a VM with the verifier silent.
#[test]
fn a_build_with_debug_assertions_verifies_by_default() {
    let want = if cfg!(debug_assertions) {
        JitVerifier::On
    } else {
        JitVerifier::Off
    };
    assert_eq!(Config::default().jit_verifier, want);
}

/// The binary and the embedding library start from one setting, as they do
/// for the JIT policy.
#[test]
fn the_cli_starts_the_verifier_where_the_library_does() {
    assert_eq!(
        parse_args(&[]).unwrap().jit_verifier,
        Config::default().jit_verifier,
        "no flag must mean what the struct Default means"
    );
}

#[test]
fn the_flag_sets_the_verifier() {
    assert_eq!(
        parse_args(&["--jit-verify=on"]).unwrap().jit_verifier,
        JitVerifier::On
    );
    assert_eq!(
        parse_args(&["--jit-verify=off"]).unwrap().jit_verifier,
        JitVerifier::Off
    );
    assert_eq!(
        parse_args(&["--jit-verify=off", "--jit-verify=on"])
            .unwrap()
            .jit_verifier,
        JitVerifier::On,
        "the last flag wins, as for every other flag"
    );
}

/// The counter-factual: a reader that took anything but `on` as off would turn
/// `--jit-verify=1` into a silent off, and a run would pass with the verifier
/// it asked for never running.
#[test]
fn the_flag_refuses_any_other_word_by_name() {
    for word in ["maybe", "", "1", "ON"] {
        let flag = format!("--jit-verify={word}");
        let err = parse_args(&[flag.as_str()]).unwrap_err();
        assert!(
            err.contains("--jit-verify") && err.contains(&format!("'{word}'")),
            "refusing {flag} must name the flag and the value, got {err:?}"
        );
    }
}
