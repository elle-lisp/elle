// audited: 2026-11-25
//! The vectors and environments a call takes and gives back, so a warm call allocates nothing.
//!
//! docs/impl/spares.md

use crate::hir::region::RuntimeRegion;
use crate::value::Value;
use std::rc::Rc;

/// How many spares each pool keeps.
pub(crate) const KEPT: usize = 64;

/// The most entries a kept vector has room for.
pub(crate) const ROOM: usize = 1024;

/// A value a pool can keep: emptied, it serves the next taker as a new one.
trait Spare: Default {
    /// Empty `self` for the next taker, and answer whether the pool may keep
    /// it: nothing else holds it, and it has room for [`ROOM`] entries at most.
    fn empty(&mut self) -> bool;
}

impl<T> Spare for Vec<T> {
    fn empty(&mut self) -> bool {
        self.clear();
        self.capacity() <= ROOM
    }
}

impl Spare for Rc<Vec<Value>> {
    fn empty(&mut self) -> bool {
        Rc::get_mut(self).is_some_and(Spare::empty)
    }
}

/// Up to [`KEPT`] spares of one kind.
struct Pool<T>(Vec<T>);

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Pool(Vec::new())
    }
}

impl<T: Spare> Pool<T> {
    fn take(&mut self) -> T {
        self.0.pop().unwrap_or_default()
    }

    fn give(&mut self, mut spare: T) {
        if self.0.len() < KEPT && spare.empty() {
            self.0.push(spare);
        }
    }
}

/// The VM's spare argument and stack buffers, environments and deferred
/// lists.
#[derive(Default)]
pub(crate) struct Spares {
    vecs: Pool<Vec<Value>>,
    envs: Pool<Rc<Vec<Value>>>,
    deferred: Pool<Vec<RuntimeRegion>>,
}

impl Spares {
    /// An empty vector: a spare, or a new one where none is kept.
    pub(crate) fn vec(&mut self) -> Vec<Value> {
        self.vecs.take()
    }

    /// Keep `spare` for the next call to [`Self::vec`], emptied, unless the
    /// pool is full or the vector has room for more than [`ROOM`] values.
    pub(crate) fn give_vec(&mut self, spare: Vec<Value>) {
        self.vecs.give(spare);
    }

    /// An empty environment nothing else holds: a spare, or a new one.
    pub(crate) fn env(&mut self) -> Rc<Vec<Value>> {
        self.envs.take()
    }

    /// Keep `spare` for the next call to [`Self::env`], emptied, unless
    /// something else still holds it, the pool is full, or it has room for
    /// more than [`ROOM`] values.
    pub(crate) fn give_env(&mut self, spare: Rc<Vec<Value>>) {
        self.envs.give(spare);
    }

    /// An empty deferred list for an activation that opens.
    pub(crate) fn deferred(&mut self) -> Vec<RuntimeRegion> {
        self.deferred.take()
    }

    /// Keep `spare` for the next activation, emptied, within the bounds.
    pub(crate) fn give_deferred(&mut self, spare: Vec<RuntimeRegion>) {
        self.deferred.give(spare);
    }
}

#[cfg(test)]
mod tests;
