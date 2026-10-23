// audited: 2026-10-23
//! Pins how the JIT verifier setting is read: the variable's two words, the
//! flag that wins over it, and the values both refuse.
//!
//! docs/config.md
//! docs/impl/verifier.md

use super::*;

/// The variable reads as unset when it is absent or empty, so an exported
/// `ELLE_JIT_VERIFY=` leaves the verifier off rather than stopping `elle`.
#[test]
fn an_unset_or_empty_variable_leaves_the_verifier_off() {
    assert_eq!(JitVerifier::from_var(None), Ok(JitVerifier::Off));
    assert_eq!(JitVerifier::from_var(Some("")), Ok(JitVerifier::Off));
}

#[test]
fn the_variable_takes_on_and_off() {
    assert_eq!(JitVerifier::from_var(Some("on")), Ok(JitVerifier::On));
    assert_eq!(JitVerifier::from_var(Some("off")), Ok(JitVerifier::Off));
}

/// The counter-factual: a reader that took anything but `on` as off would
/// turn `ELLE_JIT_VERIFY=1` into a silent off, and a suite would pass with the
/// verifier it asked for never running.
#[test]
fn the_variable_refuses_any_other_word_by_name() {
    for word in ["1", "yes", "ON", "true"] {
        let err = JitVerifier::from_var(Some(word)).unwrap_err();
        assert!(
            err.contains("ELLE_JIT_VERIFY") && err.contains(word),
            "refusing {word:?} must name the variable and the value, got {err:?}"
        );
    }
}

/// The flag is read after the variable, so it decides whatever the test
/// process's environment holds.
#[test]
fn the_flag_sets_the_verifier_over_the_variable() {
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

#[test]
fn the_flag_refuses_any_other_word_by_name() {
    for flag in ["--jit-verify=maybe", "--jit-verify="] {
        let err = parse_args(&[flag]).unwrap_err();
        assert!(
            err.contains("--jit-verify"),
            "refusing {flag} must name the flag, got {err:?}"
        );
    }
}
