use super::*;
use crate::hir::arena::BindingScope;
use crate::value::SymbolId;

/// A non-synthetic user binding (any real symbol name).
fn user(arena: &mut BindingArena) -> Binding {
    arena.alloc(SymbolId(1), BindingScope::Local)
}

/// The universal exclusion: a synthetic ANF temp holding a region is not a
/// holder, so a region it shares with a user binding stays sole-held by the
/// user binding. (Counterfactual: were synthetics admitted, `holders_of` would
/// report two holders and `sole_held` would be false.)
#[test]
fn synthetic_temps_are_never_holders() {
    let mut arena = BindingArena::new();
    let u = user(&mut arena);
    let t = arena.gensym(); // name == SymbolId::SYNTHETIC
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(u, vec![Region(2)]);
    src.insert(t, vec![Region(2)]);

    let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

    let hs = holders.holders_of(Region(2)).expect("user holder recorded");
    assert_eq!(hs.len(), 1, "synthetic temp excluded from the holder set");
    assert!(hs.contains(&u));
    assert!(
        holders.sole_held(u, Region(2)),
        "a synthetic alias does not break sole-held"
    );
}

/// The per-consumer eligibility predicate filters on top of the synthetic
/// exclusion (the reassign gate's "is read"). A user binding the predicate
/// rejects is not a holder.
#[test]
fn eligibility_predicate_excludes_rejected_bindings() {
    let mut arena = BindingArena::new();
    let kept = user(&mut arena);
    let dropped = user(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(kept, vec![Region(2)]);
    src.insert(dropped, vec![Region(2)]);

    let holders = RegionHolders::from_source_regions(&src, &arena, |b| b == kept);

    let hs = holders.holders_of(Region(2)).expect("kept holder recorded");
    assert_eq!(hs.len(), 1);
    assert!(hs.contains(&kept));
    assert!(!hs.contains(&dropped), "ineligible binding excluded");
}

/// Two distinct user bindings holding one region make it aliased: `holders_of`
/// reports both and neither sole-holds it. This is the merge seed's refusal
/// condition (`len() > 1`).
#[test]
fn two_distinct_user_holders_alias_the_region() {
    let mut arena = BindingArena::new();
    let a = user(&mut arena);
    let b = user(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(a, vec![Region(2)]);
    src.insert(b, vec![Region(2)]);

    let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

    assert_eq!(holders.holders_of(Region(2)).map(|hs| hs.len()), Some(2));
    assert!(!holders.sole_held(a, Region(2)));
    assert!(!holders.sole_held(b, Region(2)));
}

/// A region with no recorded holder is sole-held by anything, and `holders_of`
/// is `None` — the merge seed treats a holderless child as non-escaping, the
/// reassign gate treats it as sole-held.
#[test]
fn unheld_region_is_sole_held_by_anything() {
    let mut arena = BindingArena::new();
    let a = user(&mut arena);
    let holders = RegionHolders::from_source_regions(&HashMap::new(), &arena, |_| true);

    assert!(holders.holders_of(Region(9)).is_none());
    assert!(holders.sole_held(a, Region(9)));
}

/// `add` folds reassign-site regions into an index already built from
/// `binding_source_regions`, unioning per region — and re-applies the synthetic
/// exclusion so a fold-in path cannot smuggle a temp in.
#[test]
fn add_folds_in_extra_holders_and_excludes_synthetics() {
    let mut arena = BindingArena::new();
    let base = user(&mut arena);
    let extra = user(&mut arena);
    let temp = arena.gensym();
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(base, vec![Region(2)]);

    let mut holders = RegionHolders::from_source_regions(&src, &arena, |_| true);
    holders.add(extra, &arena, &[Region(2)]);
    holders.add(temp, &arena, &[Region(2)]); // excluded — synthetic

    let hs = holders.holders_of(Region(2)).expect("holders recorded");
    assert_eq!(hs.len(), 2, "base + extra, temp excluded");
    assert!(hs.contains(&base) && hs.contains(&extra));
    assert!(!hs.contains(&temp));
}

/// A duplicate region in a binding's source list does not inflate the holder
/// count — the set keeps one entry per distinct binding. This pins the
/// invariant that lets the merge seed read `len() > 1` as a *distinct*-binding
/// alias test even though it ports from a `Vec`-based map.
#[test]
fn duplicate_source_region_does_not_inflate_holder_count() {
    let mut arena = BindingArena::new();
    let b = user(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(b, vec![Region(2), Region(2)]);

    let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

    assert_eq!(holders.holders_of(Region(2)).map(|hs| hs.len()), Some(1));
    assert!(holders.sole_held(b, Region(2)));
}
