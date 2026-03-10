/// Signal registry for mapping effect keywords to bit positions.
///
/// The registry maintains a global mapping of effect keywords (`:error`, `:yield`, etc.)
/// to their corresponding bit positions. Built-in effects occupy bits 0-15, while
/// user-defined effects are allocated from bits 16-31.
use std::sync::{Mutex, OnceLock};

/// An entry in the signal registry mapping a keyword name to its bit position.
#[derive(Debug, Clone)]
pub struct SignalEntry {
    pub name: String,
    pub bit_position: u32,
}

/// Global registry mapping effect keywords to bit positions.
///
/// Built-in effects (`:error`, `:yield`, `:debug`, `:ffi`, `:halt`, `:io`) are
/// pre-registered at bits 0, 1, 2, 4, 8, 9 respectively. Bits 3, 5, 6, 7 are
/// reserved for VM-internal use and not registered.
///
/// User-defined effects are allocated starting at bit 16 and proceeding upward.
/// The registry can support up to 16 user-defined effects (bits 16-31).
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

    /// Create a registry with built-in effects pre-registered.
    ///
    /// Pre-registers:
    /// - `:error` at bit 0
    /// - `:yield` at bit 1
    /// - `:debug` at bit 2
    /// - `:ffi` at bit 4
    /// - `:halt` at bit 8
    /// - `:io` at bit 9
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        // These unwraps are safe because we're registering unique built-in names
        let _ = registry.register_builtin("error", 0);
        let _ = registry.register_builtin("yield", 1);
        let _ = registry.register_builtin("debug", 2);
        let _ = registry.register_builtin("ffi", 4);
        let _ = registry.register_builtin("halt", 8);
        let _ = registry.register_builtin("io", 9);
        registry
    }

    /// Register a built-in effect at a specific bit position.
    fn register_builtin(&mut self, name: &str, bit_position: u32) -> Result<u32, String> {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("Effect '{}' already registered", name));
        }
        self.entries.push(SignalEntry {
            name: name.to_string(),
            bit_position,
        });
        Ok(bit_position)
    }

    /// Register a user-defined effect and allocate the next available bit.
    ///
    /// Returns the bit position allocated to this effect, or an error if:
    /// - The effect name is already registered (built-in or user-defined)
    /// - All 16 user bits (16-31) are exhausted
    pub fn register(&mut self, name: &str) -> Result<u32, String> {
        // Check if already registered (built-in or user)
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("Effect '{}' already registered", name));
        }

        // Check if we've exhausted user bits (16-31)
        if self.next_user_bit > 31 {
            return Err(format!(
                "Cannot register effect '{}': all 16 user effect bits (16-31) are exhausted",
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

    /// Look up the bit position for an effect keyword.
    ///
    /// Returns `Some(bit_position)` if the effect is registered, `None` otherwise.
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

    /// Convert an effect keyword to its signal bits representation.
    ///
    /// Returns `Some(SignalBits)` if the effect is registered, `None` otherwise.
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
/// Initialized on first access with built-in effects pre-registered.
/// Thread-safe via `Mutex`.
static SIGNAL_REGISTRY: OnceLock<Mutex<SignalRegistry>> = OnceLock::new();

/// Get the global signal registry.
///
/// Returns a reference to the process-global `Mutex<SignalRegistry>`.
/// The registry is initialized with built-in effects on first access.
pub fn global_registry() -> &'static Mutex<SignalRegistry> {
    SIGNAL_REGISTRY.get_or_init(|| Mutex::new(SignalRegistry::with_builtins()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_registration() {
        let registry = SignalRegistry::with_builtins();
        assert_eq!(registry.lookup("error"), Some(0));
        assert_eq!(registry.lookup("yield"), Some(1));
        assert_eq!(registry.lookup("debug"), Some(2));
        assert_eq!(registry.lookup("ffi"), Some(4));
        assert_eq!(registry.lookup("halt"), Some(8));
        assert_eq!(registry.lookup("io"), Some(9));
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
        let bit1 = registry.register("effect1").unwrap();
        let bit2 = registry.register("effect2").unwrap();
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
        // Register 16 user effects (bits 16-31)
        for i in 0..16 {
            let name = format!("user_{}", i);
            let result = registry.register(&name);
            assert!(result.is_ok(), "Failed to register user effect {}", i);
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
