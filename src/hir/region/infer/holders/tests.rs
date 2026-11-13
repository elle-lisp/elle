// audited: 2026-11-13
//! Which bindings the holder index counts, and how it folds aliases.
//!
//! docs/impl/region/bindings.md

use super::*;
use crate::hir::arena::BindingScope;
use crate::syntax::Span;
use crate::value::SymbolId;

/// A binding the program names.
fn user(arena: &mut BindingArena) -> Binding {
    arena.alloc(SymbolId(1), BindingScope::Local)
}

/// A temp the ANF lift mints, through the lift's own constructor.
fn anf_temp(arena: &mut BindingArena) -> Binding {
    let value = Hir::silent(HirKind::Nil, Span::synthetic());
    match crate::hir::anf::name(value, arena).kind {
        HirKind::Let { bindings, .. } => bindings[0].0,
        _ => unreachable!("the ANF lift names a value with a let"),
    }
}

/// An ANF temp holding a region is no holder, so a region it shares with a
/// binding the program names stays sole-held by that binding.
/// (Counterfactual: were the temp admitted, `holders_of` would report two
/// holders and `sole_held` would be false.)
#[test]
fn an_anf_temp_is_never_a_holder() {
    let mut arena = BindingArena::new();
    let u = user(&mut arena);
    let t = anf_temp(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(u, vec![Region(2)]);
    src.insert(t, vec![Region(2)]);

    let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

    let hs = holders.holders_of(Region(2)).expect("user holder recorded");
    assert_eq!(hs.len(), 1, "the ANF temp is excluded from the holder set");
    assert!(hs.contains(&u));
    assert!(
        holders.sole_held(u, Region(2)),
        "an ANF temp does not break sole-held"
    );
}

/// A gensym another pass makes is a holder. A later node may read it many
/// times, as a loop reads a hoisted constant on every pass. (Counterfactual: a
/// rule keyed on the gensym's name drops it, so the region reads as sole-held
/// by `u` and the reassign gate donates a value `t` still reads.)
#[test]
fn a_gensym_is_a_holder() {
    let mut arena = BindingArena::new();
    let u = user(&mut arena);
    let t = arena.gensym();
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(u, vec![Region(2)]);
    src.insert(t, vec![Region(2)]);

    let holders = RegionHolders::from_source_regions(&src, &arena, |_| true);

    assert_eq!(holders.holders_of(Region(2)).map(|hs| hs.len()), Some(2));
    assert!(!holders.sole_held(u, Region(2)), "the gensym holds it too");
}

/// The per-consumer eligibility predicate filters on top of the ANF-temp
/// exclusion (the reassign gate's "is read"). A binding the predicate rejects
/// is not a holder.
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
/// `binding_source_regions`, unioning per region. It applies the ANF-temp
/// exclusion again, so a fold-in path cannot smuggle a temp in.
#[test]
fn add_folds_in_extra_holders_and_excludes_anf_temps() {
    let mut arena = BindingArena::new();
    let base = user(&mut arena);
    let extra = user(&mut arena);
    let temp = anf_temp(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(base, vec![Region(2)]);

    let mut holders = RegionHolders::from_source_regions(&src, &arena, |_| true);
    holders.add(extra, &arena, [Region(2)]);
    holders.add(temp, &arena, [Region(2)]); // excluded: an ANF temp

    let hs = holders.holders_of(Region(2)).expect("holders recorded");
    assert_eq!(hs.len(), 2, "base + extra, temp excluded");
    assert!(hs.contains(&base) && hs.contains(&extra));
    assert!(!hs.contains(&temp));
}

/// An alias entry folds the forwarded-from binding's holdings onto the binding
/// that carries them forward, so the pair reads as ONE holder and each sole-holds
/// the region. (Counterfactual: without the fold this is
/// `two_distinct_user_holders_alias_the_region`.)
#[test]
fn aliased_bindings_count_as_one_holder() {
    let mut arena = BindingArena::new();
    let from = user(&mut arena);
    let carries = user(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(from, vec![Region(2)]);
    src.insert(carries, vec![Region(2)]);
    let aliases: HashMap<Binding, Binding> = [(from, carries)].into_iter().collect();

    let holders = RegionHolders::with_aliases(&src, &arena, |_| true, aliases);

    let hs = holders.holders_of(Region(2)).expect("holders recorded");
    assert_eq!(hs.len(), 1, "the pair folds onto the carrying binding");
    assert!(hs.contains(&carries) && !hs.contains(&from));
    assert!(holders.sole_held(carries, Region(2)));
}

/// The fold applies on the `add` path too, so no consumer can reintroduce the
/// forwarded-from binding as a second holder by folding reassign-site regions in
/// after construction.
#[test]
fn add_applies_the_alias_fold() {
    let mut arena = BindingArena::new();
    let from = user(&mut arena);
    let carries = user(&mut arena);
    let aliases: HashMap<Binding, Binding> = [(from, carries)].into_iter().collect();

    let mut holders = RegionHolders::with_aliases(&HashMap::new(), &arena, |_| true, aliases);
    holders.add(from, &arena, [Region(2)]);

    let hs = holders.holders_of(Region(2)).expect("holders recorded");
    assert_eq!(hs.len(), 1);
    assert!(hs.contains(&carries));
}

/// The index resolves an alias ONE step. A chain `a → b → c` states two
/// separate one-reference claims, and `a → c` is a third claim no consumer has
/// made. So `a` folds onto `b` and stops, and `b` and `c` stay visibly aliased.
#[test]
fn alias_fold_is_one_step_not_transitive() {
    let mut arena = BindingArena::new();
    let a = user(&mut arena);
    let b = user(&mut arena);
    let c = user(&mut arena);
    let mut src: HashMap<Binding, Vec<Region>> = HashMap::new();
    src.insert(a, vec![Region(2)]);
    src.insert(c, vec![Region(2)]);
    let aliases: HashMap<Binding, Binding> = [(a, b), (b, c)].into_iter().collect();

    let holders = RegionHolders::with_aliases(&src, &arena, |_| true, aliases);

    let hs = holders.holders_of(Region(2)).expect("holders recorded");
    assert_eq!(hs.len(), 2, "a folds onto b only; c stays its own holder");
    assert!(hs.contains(&b) && hs.contains(&c));
    assert!(!holders.sole_held(c, Region(2)));
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
