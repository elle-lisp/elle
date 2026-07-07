//! Human-readable region-info dump (`super` = `hir::regions`).

use super::*;

/// Format region info as a human-readable dump string.
pub fn format_regions(
    info: &RegionInfo,
    arena: &BindingArena,
    names: &HashMap<u32, String>,
) -> String {
    use std::fmt::Write;
    let mut buf = String::new();

    fn bname(b: Binding, arena: &BindingArena, names: &HashMap<u32, String>) -> String {
        let sym = arena.get(b).name;
        let base = names
            .get(&sym.0)
            .cloned()
            .unwrap_or_else(|| format!("_{}", b.0));
        format!("{}#{}", base, b.0)
    }

    writeln!(buf, ";; ── region assignments ──").unwrap();

    // Scope regions
    let mut scopes: Vec<_> = info.scope_region.iter().collect();
    scopes.sort_by_key(|(id, _)| id.0);
    for (id, region) in &scopes {
        let live = if info.live_regions.contains(region) {
            "live"
        } else {
            "empty"
        };
        writeln!(buf, "  @{:<4} region={:<4} {}", id.0, region.0, live).unwrap();
    }

    writeln!(buf).unwrap();
    writeln!(buf, ";; ── allocation sites ──").unwrap();
    let mut allocs: Vec<_> = info.alloc_region.iter().collect();
    allocs.sort_by_key(|(id, _)| id.0);
    for (id, region) in &allocs {
        writeln!(buf, "  @{:<4} → r{}", id.0, region.0).unwrap();
    }

    writeln!(buf).unwrap();
    writeln!(buf, ";; ── binding regions ──").unwrap();
    let mut bindings: Vec<_> = info.binding_region.iter().collect();
    bindings.sort_by_key(|(b, _)| b.0);
    for (b, region) in &bindings {
        let name = bname(**b, arena, names);
        writeln!(buf, "  {:<20} → r{}", name, region.0).unwrap();
    }

    if !info.cross_region_refs.is_empty() {
        writeln!(buf).unwrap();
        writeln!(buf, ";; ── cross-region refs ──").unwrap();
        for &(site, src, dst) in &info.cross_region_refs {
            writeln!(buf, "  @{:<4} src=r{} → dst=r{}", site.0, src.0, dst.0).unwrap();
        }
    }

    if !info.region_data.is_empty() {
        writeln!(buf).unwrap();
        writeln!(buf, ";; ── decref points ──").unwrap();
        let mut data: Vec<_> = info.region_data.iter().collect();
        data.sort_by_key(|(r, _)| r.0);
        for (r, d) in &data {
            writeln!(buf, "  r{:<4} dies @{}", r.0, d.decref_point.0).unwrap();
        }
    }

    // The merge forest (docs/impl/region-model.md § Merging, § The letrec
    // closure-cycle merge): each child → its `merged_root`, tagged `[closure-cycle]`
    // for a mutual-recursion SCC/cell member (vs a builder-idiom aggregate child),
    // plus the non-member body-tail adopt sites keyed to their merged arena. The
    // permanent instrument for which cliques merged and how each is released.
    if !info.merged_parent.is_empty() || !info.cycle_tail_adopt.is_empty() {
        writeln!(buf).unwrap();
        writeln!(buf, ";; ── merge forest ──").unwrap();
        let mut merges: Vec<_> = info.merged_parent.iter().collect();
        merges.sort_by_key(|(c, _)| c.0);
        for (child, _parent) in &merges {
            let tag = if info.closure_cycle_members.contains(child) {
                " [closure-cycle]"
            } else {
                ""
            };
            writeln!(
                buf,
                "  r{} → root r{}{}",
                child.0,
                info.merged_root(**child).0,
                tag
            )
            .unwrap();
        }
        let mut sites: Vec<_> = info.cycle_tail_adopt.iter().collect();
        sites.sort_by_key(|(id, _)| id.0);
        for (site, root) in &sites {
            writeln!(buf, "  tail-adopt @{} → arena r{}", site.0, root.0).unwrap();
        }
    }

    fn set_line(buf: &mut String, label: &str, set: &rustc_hash::FxHashSet<Region>) {
        use std::fmt::Write;
        if !set.is_empty() {
            let mut rs: Vec<u32> = set.iter().map(|r| r.0).collect();
            rs.sort_unstable();
            let rs: Vec<String> = rs.into_iter().map(|r| format!("r{r}")).collect();
            writeln!(buf, "  {label}: [{}]", rs.join(" ")).unwrap();
        }
    }
    writeln!(buf).unwrap();
    set_line(&mut buf, "call-result", &info.call_result_regions);
    set_line(&mut buf, "cell-release", &info.cell_release_regions);
    set_line(
        &mut buf,
        "suppressed-decref",
        &info.suppressed_decref_regions,
    );

    writeln!(buf).unwrap();
    write!(buf, "{}", info.stats).unwrap();

    buf
}
