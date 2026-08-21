//! Region inference statistics, carried on `RegionInfo::stats`.

/// Statistics from region inference.
#[derive(Debug, Default)]
pub struct RegionStats {
    pub regions_created: usize,
    pub constraints_generated: usize,
    pub solver_iterations: usize,
    pub live_scopes: usize,
    pub empty_scopes: usize,
}

impl std::fmt::Display for RegionStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "region inference stats:")?;
        writeln!(
            f,
            "  regions: {}  constraints: {}  iterations: {}",
            self.regions_created, self.constraints_generated, self.solver_iterations
        )?;
        writeln!(
            f,
            "  live: {}  empty: {}",
            self.live_scopes, self.empty_scopes
        )?;
        Ok(())
    }
}
