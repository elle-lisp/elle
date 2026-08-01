use super::*;

/// Tree of regions induced by scope nesting.
pub(super) struct RegionTree {
    parent: HashMap<Region, Option<Region>>,
    depth: HashMap<Region, u32>,
}

#[allow(dead_code)]
impl RegionTree {
    pub(super) fn new() -> Self {
        RegionTree {
            parent: HashMap::new(),
            depth: HashMap::new(),
        }
    }

    /// Add a root region (no parent).
    pub(super) fn add_root(&mut self, r: Region) {
        self.parent.insert(r, None);
        self.depth.insert(r, 0);
    }

    /// Create a fresh root region and return it.
    pub(super) fn fresh_root(&mut self, next_region: &mut u32) -> Region {
        let r = Region(*next_region);
        *next_region += 1;
        self.add_root(r);
        r
    }

    pub(super) fn add_child(&mut self, child: Region, parent: Region) {
        self.parent.insert(child, Some(parent));
        let d = self.depth.get(&parent).copied().unwrap_or(0) + 1;
        self.depth.insert(child, d);
    }

    pub(super) fn depth_of(&self, r: Region) -> u32 {
        self.depth.get(&r).copied().unwrap_or(0)
    }

    /// Parent of a region, or None for the root.
    pub(super) fn parent_of(&self, r: Region) -> Option<Region> {
        self.parent.get(&r).copied().flatten()
    }

    /// Least common ancestor of two regions. Returns None if they
    /// share no common ancestor (should not happen in a well-formed
    /// tree with a single root).
    pub(super) fn lca(&self, mut a: Region, mut b: Region) -> Option<Region> {
        let mut da = self.depth_of(a);
        let mut db = self.depth_of(b);
        while da > db {
            a = self.parent.get(&a).copied().flatten()?;
            da -= 1;
        }
        while db > da {
            b = self.parent.get(&b).copied().flatten()?;
            db -= 1;
        }
        let mut guard = 0u32;
        while a != b {
            a = self.parent.get(&a).copied().flatten()?;
            b = self.parent.get(&b).copied().flatten()?;
            guard += 1;
            if guard > 10000 {
                return None;
            }
        }
        Some(a)
    }

    /// Is `ancestor` an ancestor-or-equal of `descendant`?
    pub(super) fn is_ancestor(&self, ancestor: Region, descendant: Region) -> bool {
        self.lca(ancestor, descendant) == Some(ancestor)
    }
}
