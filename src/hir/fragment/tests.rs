use super::*;
use crate::hir::arena::BindingScope;
use crate::hir::testkit::{HirFixture, Stage};

/// The pieces of a compiled `fn` form a fragment is built from.
struct Lambda {
    params: Vec<Binding>,
    body: Hir,
    numeric: bool,
    arena: BindingArena,
}

/// Compile `source` and hand back its first lambda's parameters, body, and
/// `(numeric!)` declaration, with the arena they name.
///
/// The trap: the fixture wraps a fragment in a `letrec` of stub lambdas unless
/// told not to, and the first lambda in the tree would then be a stub. Every
/// source here is compiled bare, so the first lambda is the one under test and
/// its free names resolve to real primitives.
fn lambda(source: &str) -> Lambda {
    let (hir, arena, _symbols) = HirFixture::new()
        .stage(Stage::Analyzed)
        .bare()
        .build(source);

    fn find(h: &Hir, out: &mut Option<(Vec<Binding>, Hir, bool)>) {
        if out.is_some() {
            return;
        }
        if let HirKind::Lambda {
            params,
            body,
            assert_numeric,
            ..
        } = &h.kind
        {
            *out = Some((params.clone(), (**body).clone(), *assert_numeric));
            return;
        }
        h.for_each_child(|c| find(c, out));
    }
    let mut out = None;
    find(&hir, &mut out);
    let (params, body, numeric) = out.expect("the source must contain a lambda");
    Lambda {
        params,
        body,
        numeric,
        arena,
    }
}

/// Every primitive in `arena`, by name — the map a graft resolves globals
/// through, built exactly as `fuse.rs` builds it.
fn primitives_by_name(arena: &BindingArena) -> FxHashMap<SymbolId, Binding> {
    let mut out = FxHashMap::default();
    for i in 0..arena.len() as u32 {
        let b = Binding(i);
        if arena.get(b).is_primitive {
            out.entry(arena.get(b).name).or_insert(b);
        }
    }
    out
}

/// An arena holding one primitive per name and nothing else — a consuming unit
/// that is nothing like the defining one.
fn arena_with_primitives(names: &[&str]) -> (BindingArena, FxHashMap<SymbolId, Binding>) {
    let mut arena = BindingArena::new();
    let mut map = FxHashMap::default();
    for name in names {
        let id = SymbolId::of(name);
        let b = arena.alloc(id, BindingScope::Local);
        let inner = arena.get_mut(b);
        inner.is_primitive = true;
        inner.is_immutable = true;
        map.insert(id, b);
    }
    (arena, map)
}

/// Every binding a body names — referenced or introduced — in walk order.
fn bindings_of(h: &Hir) -> Vec<Binding> {
    let mut out = Vec::new();
    fn walk(h: &Hir, out: &mut Vec<Binding>) {
        match &h.kind {
            HirKind::Var(b) => out.push(*b),
            HirKind::Let { bindings, .. } => out.extend(bindings.iter().map(|(b, _)| *b)),
            _ => {}
        }
        h.for_each_child(|c| walk(c, out));
    }
    walk(h, &mut out);
    out
}

/// The fragment indices of the locals a body introduces beyond its parameters.
fn let_indices(fragment: &HirFragment, params: &[u32]) -> Vec<usize> {
    fragment
        .bindings
        .iter()
        .enumerate()
        .filter(|(i, e)| !params.contains(&(*i as u32)) && matches!(e, FragmentBinding::Local(_)))
        .map(|(i, _)| i)
        .collect()
}

#[test]
fn a_graft_reproduces_the_defining_arenas_metadata_field_for_field() {
    let l = lambda("(fn [x] (let [y (hash x)] y))");
    let (params, fragment) =
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).expect("the body closes");

    let source_param = format!("{:?}", l.arena.get(l.params[0]));
    let mut host = l.arena;
    let globals = primitives_by_name(&host);
    let (map, _body) = fragment
        .graft(&mut host, &globals)
        .expect("the body grafts");

    // Debug covers every field of `BindingInner`, so a carrier that copied a
    // chosen few would differ here the moment a field it did not name matters.
    let FragmentBinding::Local(closed) = &fragment.bindings[params[0] as usize] else {
        panic!("a parameter is a local of the fragment");
    };
    assert_eq!(
        source_param,
        format!("{closed:?}"),
        "close must carry the parameter's whole metadata"
    );
    assert_eq!(
        source_param,
        format!("{:?}", host.get(map[params[0] as usize])),
        "graft must mint the parameter from that metadata"
    );

    // The `let` binding is the one a graft has no other source for: it exists
    // only inside the body.
    let lets = let_indices(&fragment, &params);
    assert_eq!(lets.len(), 1, "the body introduces one `let` binding");
    let FragmentBinding::Local(closed_let) = &fragment.bindings[lets[0]] else {
        unreachable!("filtered to locals")
    };
    assert_eq!(
        format!("{closed_let:?}"),
        format!("{:?}", host.get(map[lets[0]])),
        "graft must mint the `let` binding from its recorded metadata"
    );
}

#[test]
fn a_graft_names_only_bindings_of_the_host_arena() {
    // The trap: a `let` binding's metadata lives in the defining arena, and a
    // graft that reads it there indexes an arena it does not own. The defining
    // arena below binds every primitive, so its indices run into the hundreds;
    // the host binds one name. Reaching back panics on the index — and, where
    // it happens to be in range, silently reads a different binding.
    let l = lambda("(fn [x] (let [y (hash x)] y))");
    assert!(
        l.arena.len() > 100,
        "the defining arena must be far larger than the host, or this proves nothing"
    );
    let (_params, fragment) =
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).expect("the body closes");

    let (mut host, globals) = arena_with_primitives(&["hash"]);
    let (_map, body) = fragment
        .graft(&mut host, &globals)
        .expect("the body grafts");

    for b in bindings_of(&body) {
        assert!(
            (b.0 as usize) < host.len(),
            "the grafted body names {b:?}, outside the host arena's {} bindings",
            host.len()
        );
    }
}

#[test]
fn a_sequential_let_maps_a_later_value_to_the_earlier_fresh_binding() {
    let l = lambda("(fn [x] (let [a x] (let [b a] b)))");
    let (_params, fragment) =
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).expect("the body closes");

    let (mut host, globals) = arena_with_primitives(&[]);
    let (_map, body) = fragment
        .graft(&mut host, &globals)
        .expect("the body grafts");

    let HirKind::Let { bindings, body } = &body.kind else {
        panic!("the body is a `let`");
    };
    let outer = bindings[0].0;
    let HirKind::Let {
        bindings: inner, ..
    } = &body.kind
    else {
        panic!("the `let` body is a `let`");
    };
    assert!(
        matches!(&inner[0].1.kind, HirKind::Var(b) if *b == outer),
        "the inner value must read the outer `let`'s freshly minted binding"
    );
}

#[test]
fn a_free_local_declines_the_close() {
    // `z` is an enclosing `let` local: neither a module name nor a primitive,
    // so no other unit can name it and the body is not portable.
    let l = lambda("(let [z 1] (fn [x] z))");
    assert!(
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).is_none(),
        "a body naming an enclosing runtime local must decline"
    );
}

#[test]
fn a_letrec_declines_the_close() {
    // The rebuild introduces a `let` binding only after its own value is
    // rebuilt. That order is what makes a sequential `let` closable and a
    // `letrec` unrepresentable: a `letrec` value may name the binding it defines.
    let l = lambda("(fn [x] (letrec [y 1] y))");
    assert!(
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).is_none(),
        "a `letrec` body must decline"
    );
}

#[test]
fn an_unresolvable_global_declines_the_graft_and_mints_nothing() {
    let l = lambda("(fn [x] (hash x))");
    let (_params, fragment) =
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).expect("the body closes");

    let (mut host, globals) = arena_with_primitives(&["identical?"]);
    let before = host.len();
    assert!(
        fragment.graft(&mut host, &globals).is_none(),
        "a graft whose global does not resolve must decline"
    );
    assert_eq!(
        host.len(),
        before,
        "a declined graft must leave no half-minted locals behind"
    );
}

#[test]
fn a_fragment_survives_a_serialization_round_trip() {
    let l = lambda("(fn [x] (let [y (hash x)] y))");
    let (params, fragment) =
        HirFragment::close(&l.body, &l.params, &l.arena, l.numeric).expect("the body closes");

    let bytes = bincode::serialize(&fragment).expect("a fragment serializes");
    let decoded: HirFragment = bincode::deserialize(&bytes).expect("and deserializes");

    assert_eq!(
        format!("{:?}", fragment.bindings),
        format!("{:?}", decoded.bindings),
        "the binding table crosses the wire whole"
    );

    let (mut host, globals) = arena_with_primitives(&["hash"]);
    let (map, body) = decoded.graft(&mut host, &globals).expect("the body grafts");
    assert_eq!(map.len(), fragment.bindings.len());
    let lets = let_indices(&decoded, &params);
    assert_eq!(lets.len(), 1, "the `let` binding survives the round trip");
    for b in bindings_of(&body) {
        assert!(
            (b.0 as usize) < host.len(),
            "a decoded fragment grafts into the host arena like any other"
        );
    }
}

#[test]
fn an_intrinsic_closes_only_under_the_numeric_declaration() {
    let l = lambda("(fn [x] (numeric!) (%mul x x))");
    assert!(
        l.numeric,
        "the fixture must produce the declaration this test is about"
    );
    assert!(
        HirFragment::close(&l.body, &l.params, &l.arena, true).is_some(),
        "the declaration floors the parameters, which discharges the operand contract"
    );
    assert!(
        HirFragment::close(&l.body, &l.params, &l.arena, false).is_none(),
        "without it there is no fact to carry, so the body must decline"
    );
}
