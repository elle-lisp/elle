// audited: 2026-10-06
//! The order of a function's cases inside its dispatch loop, and the branch that reaches each case.
//!
//! docs/impl/wasm.md
//!
//! A multi-block function is `loop { block × N { br_table } }`: case `p` is the
//! code after the `p`-th `end`. A case can reach a later case with a plain `br`
//! to the `end` in front of it. Only a branch to the same or an earlier case
//! sets the dispatch local and goes round the loop. Wasmtime merges, at the
//! loop header, every value the loop carries, once for each edge that enters
//! it, so the loop takes only the edges a forward `br` cannot.

/// One case of the dispatch loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Case {
    /// The LIR block at this index.
    Block(usize),
    /// The resume entry of the park site at this index in
    /// `WasmEmitter::call_continuations`: it delivers the restart value, and
    /// runs on into the block after the site.
    Resume(usize),
}

/// How one case reaches another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Goto {
    /// A `br` of this depth lands on the target case.
    Forward(u32),
    /// Set the dispatch local to `dispatch`, then `br` this depth to the loop.
    Loop { dispatch: u32, depth: u32 },
}

/// The cases in emission order. Each resume entry follows the block that ends
/// at its park site, so it falls through into the block after the site, and
/// the block before it branches over it.
pub(super) struct Layout {
    cases: Vec<Case>,
    block_pos: Vec<usize>,
    resume_pos: Vec<usize>,
}

impl Layout {
    /// The layout of `num_blocks` blocks and one resume entry per park site,
    /// `sites` naming the block each site ends.
    pub(super) fn new(num_blocks: usize, sites: impl IntoIterator<Item = usize>) -> Layout {
        let mut resumes_after = vec![Vec::new(); num_blocks];
        let mut num_sites = 0;
        for (c, block) in sites.into_iter().enumerate() {
            resumes_after[block].push(c);
            num_sites += 1;
        }
        let mut cases = Vec::with_capacity(num_blocks + num_sites);
        let mut block_pos = vec![0; num_blocks];
        let mut resume_pos = vec![0; num_sites];
        for (b, resumes) in resumes_after.iter().enumerate() {
            block_pos[b] = cases.len();
            cases.push(Case::Block(b));
            for &c in resumes {
                resume_pos[c] = cases.len();
                cases.push(Case::Resume(c));
            }
        }
        Layout {
            cases,
            block_pos,
            resume_pos,
        }
    }

    /// The cases, in the order the emitter writes them.
    pub(super) fn cases(&self) -> &[Case] {
        &self.cases
    }

    /// The position of `case` in emission order.
    pub(super) fn position(&self, case: Case) -> usize {
        match case {
            Case::Block(b) => self.block_pos[b],
            Case::Resume(c) => self.resume_pos[c],
        }
    }

    /// The value of the dispatch local that selects `case`. The `br_table`
    /// sends value `v` to depth `v`, the `end` in front of the case at position
    /// `v`, so the value is the position.
    pub(super) fn dispatch(&self, case: Case) -> u32 {
        self.position(case) as u32
    }

    /// The `br_table` targets: value `v` branches to depth `v`.
    pub(super) fn table(&self) -> Vec<u32> {
        (0..self.cases.len() as u32).collect()
    }

    /// The depth of the dispatch loop from inside the case at position `pos`:
    /// one block for each case after it.
    pub(super) fn loop_depth(&self, pos: usize) -> u32 {
        (self.cases.len() - 1 - pos) as u32
    }

    /// The branch from inside the case at position `from`, `nesting`
    /// structured blocks deep, to `to`.
    pub(super) fn goto(&self, from: usize, to: Case, nesting: u32) -> Goto {
        let to = self.position(to);
        if to > from {
            Goto::Forward((to - from - 1) as u32 + nesting)
        } else {
            Goto::Loop {
                dispatch: to as u32,
                depth: self.loop_depth(from) + nesting,
            }
        }
    }
}

#[cfg(test)]
mod tests;
