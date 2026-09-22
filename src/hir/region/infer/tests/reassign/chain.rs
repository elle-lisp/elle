// audited: 2026-09-22
//! A chain of forwarding loop parameters hands one reference along, so its links are admitted or declined whole.
//!
//! docs/impl/region/bindings.md

use super::*;

/// Two sequential loops over one cell chain the forwarding
/// (`last#2 ← last#1 ← last#0`), and the fold follows the chain to its last
/// version: a plain `Var` init mints nothing, so the three versions hold ONE
/// reference between them. Both links take the container model, and the
/// reference has exactly one release channel at a time — the upstream link
/// FORWARDS its content drop to the link that receives the reference, keeping
/// only drop-on-overwrite for the priors it displaces.
///
/// The suppression is read over the whole chain: each link keeps its own
/// assign-value regions' decrefs (one producer release per stored value) and
/// only the shared init region is suppressed. Suppressing the upstream link's
/// value regions — which the downstream link's source set contains, because the
/// `Loop` init copies them — would leave every value that link displaced with a
/// store incref and no producer release.
#[test]
fn reassign_gate_keeps_loop_carried_cell_forwarded_from_a_cell() {
    let (hir, info) = two_loop_chain("", "0");
    let links = chain_links(&hir, &info);
    assert_eq!(
        links.len(),
        2,
        "precondition: two sequential loops give the cell two forwarding links \
         (got {links:?})"
    );
    let (up, down) = (links[0], links[1]);
    let up_regs = info.binding_source_regions[&up].clone();
    let down_regs = info.binding_source_regions[&down].clone();
    assert!(
        up_regs.iter().all(|r| down_regs.contains(r)) && down_regs.len() > up_regs.len(),
        "precondition: the downstream link's source regions are the upstream \
         link's plus its own (up={up_regs:?}, down={down_regs:?})"
    );

    let up_cell = info
        .cell_containers
        .get(&up)
        .expect("the upstream link must take the container model");
    let down_cell = info
        .cell_containers
        .get(&down)
        .expect("the downstream link must take the container model");
    assert!(
        up_cell.forwards_content,
        "the upstream link hands its content drop to the link it forwards into \
         — emitting one here would release the forwarded reference twice"
    );
    assert!(
        !down_cell.forwards_content,
        "the last link of the chain keeps the content drop: nothing forwards on \
         from it, so its final content has no other release"
    );

    // Every link's own assign-value regions keep their decrefs — those are the
    // producer releases, pinned to the store sites.
    let kept: Vec<Region> = up_cell
        .stores
        .value_regions()
        .chain(down_cell.stores.value_regions())
        .collect();
    assert!(
        kept.len() >= 2,
        "precondition: each loop stores a value region of its own (kept={kept:?})"
    );
    for r in &kept {
        assert!(
            !info.suppressed_decref_regions.contains(r),
            "region {r:?} is a link's assign-value region: suppressing it strands \
             the producer's reference of every value that link displaced",
        );
    }
    // …and the region no link stores into — the shared init, forwarded down the
    // chain uncounted — is the one that is suppressed.
    let init: Vec<Region> = up_regs
        .iter()
        .copied()
        .filter(|r| !kept.contains(r))
        .collect();
    assert_eq!(
        init.len(),
        1,
        "precondition: the chain shares exactly one init region (up={up_regs:?}, \
         kept={kept:?})"
    );
    assert!(
        info.suppressed_decref_regions.contains(&init[0]),
        "the forwarded init region's ordinary decref must be suppressed — \
         drop-on-overwrite is its one release (suppressed={:?})",
        info.suppressed_decref_regions,
    );

    for (site, b) in find_reassign_sites(&hir) {
        if b != up && b != down {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "every link of an admitted chain keeps drop-on-overwrite at @{} — \
             the channel that releases each displaced prior",
            site.0
        );
    }
}

/// An alias of a link's STORED value declines nothing: `keep` between the two
/// loops names the first loop's stored value, and its `(%length keep)` read at
/// the tail extends that region's release through the ordinary binding chain —
/// the pin rule's maximum orders the release after it, so the model's claim is
/// still only the cell's own counted reference (docs/impl/region/bindings.md
/// § "The store-site pin asks only that the store run once per binding of the
/// name it reads"). The chain is still
/// admitted or declined WHOLE; what the alias moves is the init's discharge,
/// donation to counted-init.
#[test]
fn reassign_gate_counts_an_aliased_forwarding_link() {
    let (hir, info) = two_loop_chain("(var keep last)", "(%length keep)");
    let links = chain_links(&hir, &info);
    assert!(
        links.len() >= 2,
        "precondition: the shape still chains two links (got {links:?})"
    );
    for (site, b) in find_reassign_sites(&hir) {
        if !links.contains(&b) {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "every link of the aliased chain keeps the model at @{} — the \
             alias's read extends the store-site pin instead of declining it",
            site.0
        );
    }
}

/// A chain whose LAST link is returned is admitted exactly as an unreturned one
/// is, and keeps the same one-reference-one-channel shape: the upstream link
/// forwards its content drop on, and the last link keeps it. The return claims
/// the reference the `Return`'s mint creates, not the one the chain forwards, so
/// the last link's content drop — emitted after that mint — is still the release
/// of the chain's own reference (docs/impl/region/bindings.md § "Returned
/// fn-local reassigned mutables — the return claims the MINT's reference, not
/// the cell's").
#[test]
fn reassign_gate_counts_a_forwarding_chain_whose_last_link_is_returned() {
    let (hir, info) = two_loop_chain("", "last");
    let links = chain_links(&hir, &info);
    assert_eq!(
        links.len(),
        2,
        "precondition: the shape chains two links (got {links:?})"
    );
    let up = info
        .cell_containers
        .get(&links[0])
        .expect("the upstream link takes the container model");
    let down = info
        .cell_containers
        .get(&links[1])
        .expect("the returned last link takes the container model");
    assert!(
        up.forwards_content,
        "the upstream link hands its content drop to the link it forwards into"
    );
    assert!(
        !down.forwards_content,
        "the returned last link keeps the content drop — the release of the \
         chain's one reference, which the `Return` mint has already replaced for \
         the caller"
    );
    for (site, b) in find_reassign_sites(&hir) {
        if !links.contains(&b) {
            continue;
        }
        assert!(
            info.drop_on_overwrite_sites.contains(&site),
            "every link of an admitted chain keeps drop-on-overwrite at @{}",
            site.0
        );
    }
}
