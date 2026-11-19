// audited: 2026-11-19
//! Where the epoch 18 moves put each form of a body: a unit a forward read needs lands above the read, after the units it needs.
//!
//! docs/epochs.md
//!
//! The forms are placed in order. Before a form is placed, each unit its
//! forward reads need is pulled above it, and the units that unit needs are
//! pulled first. A pull is all or nothing: it is tried on a copy, and kept
//! only when every form it moves passes nothing but quiet forms that do not
//! read its name directly. A unit of several forms is placed whole where its
//! first form stands, when its later forms may move up to it.

use super::Body;

/// The forms placed so far, in their new order.
#[derive(Clone)]
struct State {
    placed: Vec<bool>,
    order: Vec<usize>,
    /// Whether a pull placed each form above where it stood.
    pulled: Vec<bool>,
}

/// The body's forms in their new order, and whether a pull placed each one.
pub(super) fn order(body: &Body<'_>) -> (Vec<usize>, Vec<bool>) {
    let n = body.facts.len();
    let mut state = State {
        placed: vec![false; n],
        order: Vec::new(),
        pulled: vec![false; n],
    };
    for k in 0..n {
        if state.placed[k] {
            continue;
        }
        let unit = body.unit[k];
        if body.members[unit].len() > 1 {
            if let Some(next) = state.tried(|s| s.whole(body, unit, k)) {
                state = next;
                continue;
            }
        }
        for wanted in body.wants(k, &state.placed) {
            if let Some(next) = state.tried(|s| s.pull(body, wanted, &mut Vec::new())) {
                state = next;
            }
        }
        state.place(k, false);
    }
    (state.order, state.pulled)
}

impl Body<'_> {
    /// Whether every form of `unit` may move.
    fn movable_unit(&self, unit: usize) -> bool {
        self.members[unit].iter().all(|&m| self.movable[m])
    }

    /// The units `k`'s forward reads need: each one holds the first def of a
    /// name `k` reads inside a lambda, stands after `k`, and is not placed.
    fn wants(&self, k: usize, placed: &[bool]) -> Vec<usize> {
        let facts = &self.facts[k];
        let mut units: Vec<usize> = facts
            .reads
            .iter()
            .filter(|r| r.in_lambda && !facts.names.contains(&r.name))
            .filter_map(|r| self.first_def(r.name))
            .filter(|&d| d > k && !placed[d] && self.unit[d] != self.unit[k])
            .map(|d| self.unit[d])
            .collect();
        self.sorted(&mut units);
        units
    }

    /// The units that must stand above `unit` once it moves. One holds the
    /// first def of a name it reads ahead inside a lambda, or a def of a name
    /// it reads that it would move above.
    fn needs(&self, unit: usize, placed: &[bool]) -> Vec<usize> {
        let members = &self.members[unit];
        let own = |name: &str| members.iter().any(|&m| self.facts[m].names.contains(&name));
        let mut units = Vec::new();
        for &m in members {
            for read in self.facts[m].reads.iter().filter(|r| !own(r.name)) {
                if read.in_lambda {
                    if let Some(d) = self.first_def(read.name).filter(|&d| d > m && !placed[d]) {
                        units.push(self.unit[d]);
                    }
                }
                units.extend(
                    self.defs_of(read.name)
                        .filter(|&d| d < m && !placed[d] && self.unit[d] != unit)
                        .map(|d| self.unit[d]),
                );
            }
        }
        self.sorted(&mut units);
        units
    }

    /// `units` in the order of their first forms, each once.
    fn sorted(&self, units: &mut Vec<usize>) {
        units.sort_by_key(|&u| self.members[u][0]);
        units.dedup();
    }
}

impl State {
    /// A copy of the state that `step` changed, when `step` succeeds.
    fn tried(&self, step: impl FnOnce(&mut State) -> bool) -> Option<State> {
        let mut next = self.clone();
        step(&mut next).then_some(next)
    }

    fn place(&mut self, form: usize, pulled: bool) {
        self.placed[form] = true;
        self.pulled[form] = pulled;
        self.order.push(form);
    }

    /// Pull `unit` above every form not yet placed, after the units it needs.
    /// `busy` holds the units whose pull is under way.
    fn pull(&mut self, body: &Body<'_>, unit: usize, busy: &mut Vec<usize>) -> bool {
        let members = &body.members[unit];
        if busy.contains(&unit)
            || !body.movable_unit(unit)
            || members.iter().any(|&m| self.placed[m])
        {
            return false;
        }
        busy.push(unit);
        if !self.needs_placed(body, unit, busy) {
            return false;
        }
        if !members.iter().all(|&m| self.passes(body, unit, m)) {
            return false;
        }
        for &m in members {
            self.place(m, true);
        }
        true
    }

    /// Place `unit` whole where its first form `first` stands.
    fn whole(&mut self, body: &Body<'_>, unit: usize, first: usize) -> bool {
        let members = &body.members[unit];
        if !body.movable_unit(unit) || members.iter().any(|&m| self.placed[m]) {
            return false;
        }
        let mut busy = vec![unit];
        if !self.needs_placed(body, unit, &mut busy) {
            return false;
        }
        let later = members.iter().filter(|&&m| m != first);
        if !later.clone().all(|&m| self.passes(body, unit, m)) {
            return false;
        }
        self.place(first, false);
        for &m in later {
            self.place(m, true);
        }
        true
    }

    /// Pull each unit `unit` needs that is not placed yet.
    fn needs_placed(&mut self, body: &Body<'_>, unit: usize, busy: &mut Vec<usize>) -> bool {
        for need in body.needs(unit, &self.placed) {
            if body.members[need].iter().all(|&m| self.placed[m]) {
                continue;
            }
            if !self.pull(body, need, busy) {
                return false;
            }
        }
        true
    }

    /// Whether `form`, of `unit`, may move above every form before it not
    /// yet placed: each is quiet and does not read its name directly.
    fn passes(&self, body: &Body<'_>, unit: usize, form: usize) -> bool {
        let name = body.facts[form].names[0];
        (0..form)
            .filter(|&f| !self.placed[f] && body.unit[f] != unit)
            .all(|f| {
                let facts = &body.facts[f];
                facts.quiet && !facts.reads.iter().any(|r| !r.in_lambda && r.name == name)
            })
    }
}
