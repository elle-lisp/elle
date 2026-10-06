// audited: 2026-10-06
//! Freezing: copying a lowered `LirFunction` into the plain records every reader reads.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

use super::owned::{FrozenModule, LirOwned};
use crate::lir::{LirFunction, LirModule};

/// Freeze one function.
pub fn freeze(_func: &LirFunction) -> Result<LirOwned, String> {
    Err("freeze: not built".to_string())
}

impl LirModule {
    /// Freeze the entry function and every closure.
    pub fn freeze(&self) -> Result<FrozenModule, String> {
        Ok(FrozenModule {
            entry: freeze(&self.entry)?,
            closures: self.closures.iter().map(freeze).collect::<Result<_, _>>()?,
        })
    }
}
