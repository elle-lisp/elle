// audited: 2026-11-10
//! What a call through a merged binding knows: the signal, arity and bounds every binding it may stand for shares.
//!
//! docs/signals/inference.md
//!
//! A merge in a file may close before the fixpoint settles the signals of the
//! bindings it stands for, so the fixpoint joins every merge again after a
//! signal moves.

use super::super::*;

/// A merged binding, and each binding its merge var may hold when the binding
/// is defined. `None` stands for nil.
pub(in crate::hir::analyze) struct Join {
    merged: Binding,
    inputs: Vec<Option<Binding>>,
}

/// The value every item holds, when there is one.
fn shared<T: PartialEq>(mut items: impl Iterator<Item = Option<T>>) -> Option<T> {
    let first = items.next()??;
    items
        .all(|item| item.as_ref() == Some(&first))
        .then_some(first)
}

/// Make `map` hold `value` for `binding`, or nothing when `value` is `None`.
fn put<T>(map: &mut HashMap<Binding, T>, binding: Binding, value: Option<T>) {
    match value {
        Some(value) => map.insert(binding, value),
        None => map.remove(&binding),
    };
}

impl<'a> Analyzer<'a> {
    /// Record that `merged` stands for one of `inputs`, and join their facts.
    pub(super) fn join_merge(&mut self, merged: Binding, inputs: Vec<Option<Binding>>) {
        let join = Join { merged, inputs };
        self.join_facts(&join);
        self.defs.joins.push(join);
    }

    /// Join every merge again, in the order the merges closed, so a merge that
    /// stands for an earlier merged binding reads that binding's new facts.
    pub(in crate::hir::analyze) fn rejoin_merges(&mut self) {
        let joins = std::mem::take(&mut self.defs.joins);
        for join in &joins {
            self.join_facts(join);
        }
        self.defs.joins = joins;
    }

    /// Give `join.merged` the facts every one of its inputs shares. A nil input
    /// is no function, so a merge that may be nil knows none. A call through
    /// the merge checks keyword arguments when any input collects them.
    fn join_facts(&mut self, join: &Join) {
        let inputs: Vec<Binding> = join
            .inputs
            .iter()
            .copied()
            .collect::<Option<_>>()
            .unwrap_or_default();
        let merged = join.merged;
        let signal = if inputs.is_empty() {
            None
        } else {
            inputs.iter().try_fold(Signal::silent(), |signal, input| {
                self.signal_env.get(input).map(|s| signal.combine(*s))
            })
        };
        let arity = shared(inputs.iter().map(|b| self.arity_env.get(b).copied()));
        let bound_key = |b: &Binding| -> Vec<_> {
            self.bound_env.get(b).map_or_else(Vec::new, |bounds| {
                bounds.iter().map(|p| (p.position, p.allowed)).collect()
            })
        };
        let bounds = shared(inputs.iter().map(|b| Some(bound_key(b))))
            .filter(|key| !key.is_empty())
            .and_then(|_| self.bound_env.get(&inputs[0]).cloned());
        let collects = inputs.iter().any(|b| self.keyword_collectors.contains(b));
        put(&mut self.signal_env, merged, signal);
        put(&mut self.arity_env, merged, arity);
        put(&mut self.bound_env, merged, bounds);
        if collects {
            self.keyword_collectors.insert(merged);
        } else {
            self.keyword_collectors.remove(&merged);
        }
    }
}
