use super::{
    SIG_DEBUG, SIG_ERROR, SIG_EXEC, SIG_FFI, SIG_FUEL, SIG_HALT, SIG_IO, SIG_WAIT, SIG_YIELD,
};
/// Signal registry for mapping signal keywords to bit positions.
///
/// The registry maintains a global mapping of signal keywords (`:error`, `:yield`, etc.)
/// to their corresponding bit positions. Built-in signals occupy bits 0-15, while
/// user-defined signals are allocated from bits 16-31.
use std::sync::{Mutex, OnceLock};

/// An entry in the signal registry mapping a keyword name to its bit position.
#[derive(Debug, Clone)]
pub struct SignalEntry {
    pub name: String,
    pub bit_position: u32,
}

/// Global registry mapping signal keywords to bit positions.
///
/// Built-in signals (`:error`, `:yield`, `:debug`, `:ffi`, `:halt`, `:io`, `:exec`, `:fuel`) are
/// pre-registered at bits 0, 1, 2, 4, 8, 9, 11, 12 respectively. Bits 3, 5, 6, 7, 10 are
/// reserved for VM-internal use and not registered.
///
/// User-defined signals are allocated starting at bit 16 and proceeding upward.
/// The registry can support up to 16 user-defined signals (bits 16-31).
pub struct SignalRegistry {
    entries: Vec<SignalEntry>,
    next_user_bit: u32,
}

impl SignalRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        SignalRegistry {
            entries: Vec::new(),
            next_user_bit: 16,
        }
    }

    /// Create a registry with built-in signals pre-registered.
    ///
    /// Pre-registers:
    /// - `:error` at bit 0
    /// - `:yield` at bit 1
    /// - `:debug` at bit 2
    /// - `:ffi` at bit 4
    /// - `:halt` at bit 8
    /// - `:io` at bit 9
    /// - `:exec` at bit 11
    /// - `:fuel` at bit 12
    /// - `:wait` at bit 14
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        // These unwraps are safe because we're registering unique built-in names
        let _ = registry.register_builtin("error", SIG_ERROR.0.trailing_zeros());
        let _ = registry.register_builtin("yield", SIG_YIELD.0.trailing_zeros());
        let _ = registry.register_builtin("debug", SIG_DEBUG.0.trailing_zeros());
        let _ = registry.register_builtin("ffi", SIG_FFI.0.trailing_zeros());
        let _ = registry.register_builtin("halt", SIG_HALT.0.trailing_zeros());
        let _ = registry.register_builtin("io", SIG_IO.0.trailing_zeros());
        let _ = registry.register_builtin("exec", SIG_EXEC.0.trailing_zeros());
        let _ = registry.register_builtin("fuel", SIG_FUEL.0.trailing_zeros());
        let _ = registry.register_builtin("wait", SIG_WAIT.0.trailing_zeros());
        registry
    }

    /// Register a built-in signal at a specific bit position.
    fn register_builtin(&mut self, name: &str, bit_position: u32) -> Result<u32, String> {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("Signal '{}' already registered", name));
        }
        self.entries.push(SignalEntry {
            name: name.to_string(),
            bit_position,
        });
        Ok(bit_position)
    }

    /// Register a user-defined signal and allocate the next available bit.
    ///
    /// Returns the bit position allocated to this signal, or an error if:
    /// - The signal name is already registered (built-in or user-defined)
    /// - All 16 user bits (16-31) are exhausted
    pub fn register(&mut self, name: &str) -> Result<u32, String> {
        // Check if already registered (built-in or user)
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("Signal '{}' already registered", name));
        }

        // Check if we've exhausted user bits (16-31)
        if self.next_user_bit > 31 {
            return Err(format!(
                "Cannot register signal '{}': all 16 user signal bits (16-31) are exhausted",
                name
            ));
        }

        let bit_position = self.next_user_bit;
        self.entries.push(SignalEntry {
            name: name.to_string(),
            bit_position,
        });
        self.next_user_bit += 1;
        Ok(bit_position)
    }

    /// Look up the bit position for an signal keyword.
    ///
    /// Returns `Some(bit_position)` if the signal is registered, `None` otherwise.
    pub fn lookup(&self, name: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.bit_position)
    }

    /// Get all registered entries.
    pub fn entries(&self) -> &[SignalEntry] {
        &self.entries
    }

    /// Convert an signal keyword to its signal bits representation.
    ///
    /// Returns `Some(SignalBits)` if the signal is registered, `None` otherwise.
    pub fn to_signal_bits(&self, name: &str) -> Option<crate::value::fiber::SignalBits> {
        self.lookup(name)
            .map(|bit_pos| crate::value::fiber::SignalBits(1 << bit_pos))
    }

    /// Format signal bits as a human-readable string.
    ///
    /// Returns a string like `"{:error, :yield}"` for multiple bits, or `"{}"` for empty.
    pub fn format_signal_bits(&self, bits: crate::value::fiber::SignalBits) -> String {
        let mut names = Vec::new();
        for entry in &self.entries {
            if bits.0 & (1 << entry.bit_position) != 0 {
                names.push(format!(":{}", entry.name));
            }
        }

        if names.is_empty() {
            "{}".to_string()
        } else {
            format!("{{{}}}", names.join(", "))
        }
    }
}

impl Default for SignalRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global signal registry singleton.
///
/// Initialized on first access with built-in signals pre-registered.
/// Thread-safe via `Mutex`.
static SIGNAL_REGISTRY: OnceLock<Mutex<SignalRegistry>> = OnceLock::new();

/// Get the global signal registry.
///
/// Returns a reference to the process-global `Mutex<SignalRegistry>`.
/// The registry is initialized with built-in signals on first access.
pub fn global_registry() -> &'static Mutex<SignalRegistry> {
    SIGNAL_REGISTRY.get_or_init(|| Mutex::new(SignalRegistry::with_builtins()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_registration() {
        let registry = SignalRegistry::with_builtins();
        assert_eq!(registry.lookup("error"), Some(SIG_ERROR.0.trailing_zeros()));
        assert_eq!(registry.lookup("yield"), Some(SIG_YIELD.0.trailing_zeros()));
        assert_eq!(registry.lookup("debug"), Some(SIG_DEBUG.0.trailing_zeros()));
        assert_eq!(registry.lookup("ffi"), Some(SIG_FFI.0.trailing_zeros()));
        assert_eq!(registry.lookup("halt"), Some(SIG_HALT.0.trailing_zeros()));
        assert_eq!(registry.lookup("io"), Some(SIG_IO.0.trailing_zeros()));
        assert_eq!(registry.lookup("fuel"), Some(SIG_FUEL.0.trailing_zeros()));
    }

    #[test]
    fn test_user_registration() {
        let mut registry = SignalRegistry::with_builtins();
        let bit = registry.register("heartbeat").unwrap();
        assert_eq!(bit, 16);
        assert_eq!(registry.lookup("heartbeat"), Some(16));
    }

    #[test]
    fn test_user_registration_sequential() {
        let mut registry = SignalRegistry::with_builtins();
        let bit1 = registry.register("signal1").unwrap();
        let bit2 = registry.register("signal2").unwrap();
        assert_eq!(bit1, 16);
        assert_eq!(bit2, 17);
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
        // Register 16 user signals (bits 16-31)
        for i in 0..16 {
            let name = format!("user_{}", i);
            let result = registry.register(&name);
            assert!(result.is_ok(), "Failed to register user signal {}", i);
        }
        // 17th should fail
        let result = registry.register("user_16");
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
        assert_eq!(bits.0, 1 << 0);
    }

    #[test]
    fn test_format_signal_bits_single() {
        let registry = SignalRegistry::with_builtins();
        let bits = crate::value::fiber::SignalBits(1 << 0); // error bit
        let formatted = registry.format_signal_bits(bits);
        assert!(formatted.contains(":error"));
    }

    #[test]
    fn test_format_signal_bits_multiple() {
        let registry = SignalRegistry::with_builtins();
        let bits = crate::value::fiber::SignalBits((1 << 0) | (1 << 1)); // error and yield
        let formatted = registry.format_signal_bits(bits);
        assert!(formatted.contains(":error"));
        assert!(formatted.contains(":yield"));
    }

    #[test]
    fn test_format_signal_bits_empty() {
        let registry = SignalRegistry::with_builtins();
        let bits = crate::value::fiber::SignalBits(0);
        let formatted = registry.format_signal_bits(bits);
        assert_eq!(formatted, "{}");
    }

    #[test]
    fn test_global_registry_returns_same_instance() {
        let reg1 = global_registry();
        let reg2 = global_registry();
        assert_eq!(reg1 as *const _, reg2 as *const _);
    }
}
