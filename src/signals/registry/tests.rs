//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_builtin_registration() {
    let registry = SignalRegistry::with_builtins();
    assert_eq!(registry.lookup("error"), Some(SIG_ERROR.trailing_zeros()));
    assert_eq!(registry.lookup("yield"), Some(SIG_YIELD.trailing_zeros()));
    assert_eq!(registry.lookup("debug"), Some(SIG_DEBUG.trailing_zeros()));
    assert_eq!(registry.lookup("ffi"), Some(SIG_FFI.trailing_zeros()));
    assert_eq!(registry.lookup("halt"), Some(SIG_HALT.trailing_zeros()));
    assert_eq!(registry.lookup("io"), Some(SIG_IO.trailing_zeros()));
    assert_eq!(registry.lookup("fuel"), Some(SIG_FUEL.trailing_zeros()));
}

#[test]
fn test_user_registration() {
    let mut registry = SignalRegistry::with_builtins();
    let bit = registry.register("heartbeat").unwrap();
    assert_eq!(bit, 32);
    assert_eq!(registry.lookup("heartbeat"), Some(32));
}

#[test]
fn test_user_registration_sequential() {
    let mut registry = SignalRegistry::with_builtins();
    let bit1 = registry.register("signal1").unwrap();
    let bit2 = registry.register("signal2").unwrap();
    assert_eq!(bit1, 32);
    assert_eq!(bit2, 33);
}

#[test]
fn test_duplicate_registration_error() {
    let mut registry = SignalRegistry::with_builtins();
    let _ = registry.register("heartbeat").unwrap();
    let result = registry.register("heartbeat");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already registered"));
}

#[test]
fn test_builtin_not_shadowed() {
    let mut registry = SignalRegistry::with_builtins();
    let result = registry.register("error");
    assert!(result.is_err());
}

#[test]
fn test_overflow() {
    let mut registry = SignalRegistry::with_builtins();
    // Register 32 user signals (bits 32-63)
    for i in 0..32 {
        let name = format!("user_{}", i);
        let result = registry.register(&name);
        assert!(result.is_ok(), "Failed to register user signal {}", i);
    }
    // 33rd should fail
    let result = registry.register("user_32");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("exhausted"));
}

#[test]
fn test_lookup_unknown() {
    let registry = SignalRegistry::with_builtins();
    assert_eq!(registry.lookup("nonexistent"), None);
}

#[test]
fn test_to_signal_bits() {
    let registry = SignalRegistry::with_builtins();
    let bits = registry.to_signal_bits("error").unwrap();
    assert_eq!(bits, crate::value::fiber::SignalBits::from_bit(0));
}

#[test]
fn test_format_signal_bits_single() {
    let registry = SignalRegistry::with_builtins();
    let bits = crate::value::fiber::SignalBits::from_bit(0); // error bit
    let formatted = registry.format_signal_bits(bits);
    assert!(formatted.contains(":error"));
}

#[test]
fn test_format_signal_bits_multiple() {
    let registry = SignalRegistry::with_builtins();
    let bits = crate::value::fiber::SignalBits::from_bit(0)
        .union(crate::value::fiber::SignalBits::from_bit(1)); // error and yield
    let formatted = registry.format_signal_bits(bits);
    assert!(formatted.contains(":error"));
    assert!(formatted.contains(":yield"));
}

#[test]
fn test_format_signal_bits_empty() {
    let registry = SignalRegistry::with_builtins();
    let bits = crate::value::fiber::SignalBits::EMPTY;
    let formatted = registry.format_signal_bits(bits);
    assert_eq!(formatted, "{}");
}

#[test]
fn test_global_registry_returns_same_instance() {
    let reg1 = global_registry();
    let reg2 = global_registry();
    assert_eq!(reg1 as *const _, reg2 as *const _);
}
