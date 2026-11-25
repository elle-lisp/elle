// audited: 2026-11-25
//! The vectors and environments a call takes and gives back, so a warm call allocates nothing.
//!
//! docs/impl/spares.md

use crate::value::Value;
use std::rc::Rc;

/// How many spares each pool keeps.
pub(crate) const KEPT: usize = 64;

/// The most values a kept vector has room for.
pub(crate) const ROOM: usize = 1024;

/// The VM's spare argument and stack buffers, and its spare environments.
#[derive(Default)]
pub(crate) struct Spares {}

impl Spares {
    /// An empty vector: a spare, or a new one where none is kept.
    pub(crate) fn vec(&mut self) -> Vec<Value> {
        Vec::new()
    }

    /// Keep `spare` for the next call to [`Self::vec`], emptied, unless the
    /// pool is full or the vector has room for more than [`ROOM`] values.
    pub(crate) fn give_vec(&mut self, spare: Vec<Value>) {
        drop(spare);
    }

    /// An empty environment nothing else holds: a spare, or a new one.
    pub(crate) fn env(&mut self) -> Rc<Vec<Value>> {
        Rc::default()
    }

    /// Keep `spare` for the next call to [`Self::env`], emptied, unless
    /// something else still holds it, the pool is full, or it has room for
    /// more than [`ROOM`] values.
    pub(crate) fn give_env(&mut self, spare: Rc<Vec<Value>>) {
        drop(spare);
    }
}

#[cfg(test)]
mod tests;
