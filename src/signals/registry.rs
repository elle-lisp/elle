use super::{
    SIG_DEBUG, SIG_ERROR, SIG_EXEC, SIG_FFI, SIG_FUEL, SIG_GPU, SIG_HALT, SIG_IO, SIG_OS_SIGNAL,
    SIG_WAIT, SIG_YIELD,
};
/// Signal registry for mapping signal keywords to bit positions.
///
/// The registry maintains a global mapping of signal keywords (`:error`, `:yield`, etc.)
/// to their corresponding bit positions. Built-in signals occupy bits 0-15,
/// bits 16-31 are runtime-reserved, and user-defined signals are allocated
/// from bits 32-63.
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
/// User-defined signals are allocated starting at bit 32 and proceeding upward.
/// The registry can support up to 32 user-defined signals (bits 32-63).
#[derive(Clone)]
pub struct SignalRegistry {
    entries: Vec<SignalEntry>,
    next_user_bit: u32,
}

impl SignalRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        SignalRegistry {
            entries: Vec::new(),
            next_user_bit: 32,
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
        let _ = registry.register_builtin("error", SIG_ERROR.trailing_zeros());
        let _ = registry.register_builtin("yield", SIG_YIELD.trailing_zeros());
        let _ = registry.register_builtin("debug", SIG_DEBUG.trailing_zeros());
        let _ = registry.register_builtin("ffi", SIG_FFI.trailing_zeros());
        let _ = registry.register_builtin("halt", SIG_HALT.trailing_zeros());
        let _ = registry.register_builtin("io", SIG_IO.trailing_zeros());
        let _ = registry.register_builtin("exec", SIG_EXEC.trailing_zeros());
        let _ = registry.register_builtin("fuel", SIG_FUEL.trailing_zeros());
        let _ = registry.register_builtin("wait", SIG_WAIT.trailing_zeros());
        let _ = registry.register_builtin("gpu", SIG_GPU.trailing_zeros());
        let _ = registry.register_builtin("os-signal", SIG_OS_SIGNAL.trailing_zeros());
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
    /// - All 32 user bits (32-63) are exhausted
    pub fn register(&mut self, name: &str) -> Result<u32, String> {
        // Check if already registered (built-in or user)
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("Signal '{}' already registered", name));
        }

        // Check if we've exhausted user bits (32-63)
        if self.next_user_bit > 63 {
            return Err(format!(
                "Cannot register signal '{}': all 32 user signal bits (32-63) are exhausted",
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

    /// Register a user signal, or return its existing bit if already registered.
    ///
    /// Idempotent for USER signals (bits 32-63): re-declaring one across separate
    /// compilations reuses its bit instead of erroring. Built-in/reserved signals
    /// (bits 0-31) still error on re-declaration — a program cannot redefine
    /// `:error`, `:yield`, etc. Intra-compilation duplicate detection is the
    /// caller's job (see `Analyzer::declare_signal`); this only dedups across
    /// compiles that share the process-global registry.
    pub fn register_or_get(&mut self, name: &str) -> Result<u32, String> {
        if let Some(entry) = self.entries.iter().find(|e| e.name == name) {
            if entry.bit_position < 32 {
                return Err(format!("Signal '{}' already registered", name));
            }
            return Ok(entry.bit_position);
        }
        self.register(name)
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
            .map(crate::value::fiber::SignalBits::from_bit)
    }

    /// Convert signal bits to a Vec of keyword Values.
    ///
    /// Used by `fiber/caps` and capability denial payloads to produce
    /// keyword sets from signal bitmasks.
    pub fn bits_to_keywords(
        &self,
        bits: crate::value::fiber::SignalBits,
    ) -> Vec<crate::value::Value> {
        self.entries
            .iter()
            .filter(|e| bits.has_bit(e.bit_position))
            .map(|e| crate::value::Value::keyword(&e.name))
            .collect()
    }

    /// Format signal bits as a human-readable string.
    ///
    /// Returns a string like `"{:error, :yield}"` for multiple bits, or `"{}"` for empty.
    pub fn format_signal_bits(&self, bits: crate::value::fiber::SignalBits) -> String {
        let mut names = Vec::new();
        for entry in &self.entries {
            if bits.has_bit(entry.bit_position) {
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

/// Execute a closure with shared access to the global signal registry.
///
/// Recovers from mutex poisoning (which can occur if a thread panics while
/// holding the lock) by accessing the inner value regardless. This prevents
/// a single panic from permanently disabling signal registry access across
/// the entire process.
///
/// All read operations on the registry should use this function instead of
/// `global_registry().lock().unwrap()`.
pub fn with_registry<F, R>(f: F) -> R
where
    F: FnOnce(&SignalRegistry) -> R,
{
    let guard = global_registry().lock().unwrap_or_else(|e| e.into_inner());
    f(&guard)
}

/// Clone the current global registry state.
///
/// Paired with [`restore_registry`] to bracket a *diagnostic* compile that must
/// not leave observable global side effects — e.g. `compile/dumps` / `render_all`
/// renders a module's `--dump` artifacts by compiling it, and a `(signal :kw)`
/// declaration in that module would otherwise permanently register the signal,
/// colliding with a later compile of the same source. See `src/dump.rs`.
pub fn snapshot_registry() -> SignalRegistry {
    with_registry(|r| r.clone())
}

/// Restore the global registry to a previously captured [`snapshot_registry`]
/// state. Poison-tolerant, like [`with_registry`].
pub fn restore_registry(snapshot: SignalRegistry) {
    let mut guard = global_registry().lock().unwrap_or_else(|e| e.into_inner());
    *guard = snapshot;
}

#[cfg(test)]
mod tests;
