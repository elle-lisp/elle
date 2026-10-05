// audited: 2026-10-04
//! How a `Signal` prints: its class, then the capability flags it carries.
//!
//! docs/signals/protocol.md

use super::{Signal, SIG_DEBUG, SIG_ERROR, SIG_FFI, SIG_HALT, SIG_IO, SIG_YIELD};
use std::fmt;

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.propagates != 0 {
            let indices: Vec<_> = self.propagated_params().map(|i| i.to_string()).collect();
            write!(f, "polymorphic({})", indices.join(","))?;
        } else if self.bits.intersects(SIG_YIELD) {
            write!(f, "yields")?;
        } else if self.bits.intersects(SIG_IO) {
            // An async primitive raises `:io` and no longer claims `:yield`, so
            // without this arm every port and socket signal would print as
            // "silent" — the one word it is not.
            write!(f, "io")?;
        } else {
            write!(f, "silent")?;
        }

        // Append capability flags
        let mut flags = Vec::new();
        if self.bits.intersects(SIG_ERROR) {
            flags.push("errors");
        }
        if self.bits.intersects(SIG_HALT) {
            flags.push("halts");
        }
        if self.bits.intersects(SIG_FFI) {
            flags.push("ffi");
        }
        if self.bits.intersects(SIG_DEBUG) {
            flags.push("debug");
        }
        if !flags.is_empty() {
            write!(f, "+{}", flags.join("+"))?;
        }
        Ok(())
    }
}
