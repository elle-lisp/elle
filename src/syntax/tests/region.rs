//! The claims that make the tree region-native (docs/impl/syntax.md).

use crate::syntax::{ScopeId, Span, Syntax, SyntaxArena, SyntaxHeap, SyntaxKind};

/// Which region owns the bytes at `ptr`, as the heap sees it.
fn owner_of(arena: &SyntaxArena, ptr: *const u8) -> u32 {
    unsafe { (*arena.heap_ptr()).region_of_ptr(ptr as *const ()) }
}

#[test]
fn a_syntax_node_is_copy_pod_that_needs_no_drop() {
    // The whole point of the migration: page bytes ARE the node. A `Vec`,
    // `Box`, `Rc`, or `String` anywhere inside would put a Rust-heap
    // allocation in a region page, which no image can dump and no region
    // teardown can reclaim.
    fn assert_copy<T: Copy>() {}
    assert_copy::<Syntax>();
    assert_copy::<SyntaxKind>();
    assert!(!std::mem::needs_drop::<Syntax>());
    assert_eq!(std::mem::size_of::<Syntax>(), 64);
}

#[test]
fn a_compounds_children_live_in_the_arenas_region() {
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let span = Span::new(0, 1, 1, 1);

    let items = [
        Syntax::new(SyntaxKind::Int(1), span),
        Syntax::new(SyntaxKind::Int(2), span),
    ];
    let list = Syntax::list(&arena, &items, span);

    let kids = list.kind.children();
    assert_eq!(kids.len(), 2);
    assert_eq!(
        owner_of(&arena, kids.as_ptr() as *const u8),
        arena.region().get(),
        "a list's children must be region data, not a Vec"
    );
}

#[test]
fn a_names_bytes_live_in_the_arenas_region() {
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let sym = Syntax::symbol(&arena, "quite-a-long-symbol-name", Span::synthetic());

    let SyntaxKind::Symbol(name) = sym.kind else {
        panic!("expected a symbol");
    };
    assert_eq!(name.as_str(), "quite-a-long-symbol-name");
    assert_eq!(
        owner_of(&arena, name.bytes().as_ptr()),
        arena.region().get(),
        "a symbol's spelling must be region data, not a String"
    );
}

#[test]
fn payloads_deref_to_str_slice_and_node() {
    // The seam every reader of the tree stands on: a name reads as `&str`,
    // children read as `&[Syntax]`, and a wrapped form reads as `&Syntax`, so
    // matching and walking the tree needs no knowledge of regions.
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let span = Span::synthetic();

    let sym = Syntax::symbol(&arena, "foo", span);
    assert_eq!(sym.as_symbol(), Some("foo"));
    assert!(sym.is_symbol("foo"));
    assert!(!sym.is_symbol("bar"));

    let list = Syntax::list(&arena, &[sym, Syntax::new(SyntaxKind::Int(7), span)], span);
    let items: &[Syntax] = list.as_list().expect("a list");
    assert_eq!(items.len(), 2);
    assert!(matches!(items[1].kind, SyntaxKind::Int(7)));

    let quoted = Syntax::quote(&arena, sym, span);
    let SyntaxKind::Quote(inner) = quoted.kind else {
        panic!("expected a quote");
    };
    let inner: &Syntax = &inner;
    assert_eq!(inner.as_symbol(), Some("foo"));
}

#[test]
fn a_copy_into_another_arena_shares_nothing_with_its_source() {
    // The operation that lets a tree cross an arena boundary — out of a
    // transformer's scratch, into the working arena, into a Value's region.
    // If it shared a slice or a string, the destination would hold a pointer
    // into a region that is about to be freed.
    let mut source_home = SyntaxHeap::new();
    let source = source_home.arena();
    let mut dest_home = SyntaxHeap::new();
    let dest = dest_home.arena();
    let span = Span::new(0, 3, 1, 1);

    let inner = Syntax::symbol(&source, "inner-name", span);
    let tree = Syntax::list(
        &source,
        &[inner, Syntax::new(SyntaxKind::Float(1.5), span)],
        span,
    );

    let copy = tree.copy_into(&dest);

    let kids = copy.kind.children();
    assert_eq!(
        owner_of(&dest, kids.as_ptr() as *const u8),
        dest.region().get()
    );
    let SyntaxKind::Symbol(name) = kids[0].kind else {
        panic!("expected a symbol");
    };
    assert_eq!(name.as_str(), "inner-name");
    assert_eq!(owner_of(&dest, name.bytes().as_ptr()), dest.region().get());
}

#[test]
fn a_stamped_copy_does_not_disturb_its_source() {
    // Subtrees are shared by pointer, so a scope walk MUST copy. The
    // counter-factual: walking in place would stamp the macro argument the
    // caller still holds, and the caller's identifier would resolve in a
    // scope it never entered.
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let span = Span::synthetic();

    let child = Syntax::symbol(&arena, "x", span);
    let source = Syntax::list(&arena, &[child], span);

    let mut copy = source.copy_into(&arena);
    copy.add_scope(&arena, ScopeId(4));
    copy.children_mut()[0].add_scope(&arena, ScopeId(4));

    assert_eq!(copy.scopes(), &[ScopeId(4)]);
    assert_eq!(copy.kind.children()[0].scopes(), &[ScopeId(4)]);
    assert!(source.scopes().is_empty());
    assert!(
        source.kind.children()[0].scopes().is_empty(),
        "the source tree must not see the copy's scope"
    );
}

#[test]
fn scope_sets_grow_without_losing_a_scope() {
    // The scope slice has no inline capacity: every add reallocates. Measured
    // sets hold three scopes or fewer, but nothing enforces that, so growth
    // past any size must keep every member and stay idempotent.
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let mut node = Syntax::symbol(&arena, "x", Span::synthetic());

    for i in 0..12u32 {
        node.add_scope(&arena, ScopeId(i));
    }
    assert_eq!(node.scopes().len(), 12);
    for i in 0..12u32 {
        assert!(node.scopes().contains(&ScopeId(i)));
    }

    node.add_scope(&arena, ScopeId(5));
    assert_eq!(node.scopes().len(), 12, "adding a present scope is a no-op");

    node.flip_scope(&arena, ScopeId(5));
    assert_eq!(node.scopes().len(), 11);
    assert!(!node.scopes().contains(&ScopeId(5)));
    node.flip_scope(&arena, ScopeId(5));
    assert!(node.scopes().contains(&ScopeId(5)));
}

#[test]
fn a_syntax_literal_reports_no_children_to_a_generic_walk() {
    // `SyntaxLiteral` carries the scopes of the context it was captured in,
    // so the scope walks must not descend into it. Reporting no children is
    // how the shared `children`/`rebuild` pair states that, and every walk
    // built on them inherits it.
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let span = Span::synthetic();

    let captured = Syntax::symbol_scoped(&arena, "it", span, &[ScopeId(9)]);
    let literal = Syntax::new(SyntaxKind::SyntaxLiteral(arena.node(captured)), span);
    assert!(literal.kind.children().is_empty());

    // The copy still carries the captured node and its scopes.
    let copy = literal.copy_into(&arena);
    let SyntaxKind::SyntaxLiteral(inner) = copy.kind else {
        panic!("expected a syntax literal");
    };
    assert_eq!(inner.as_symbol(), Some("it"));
    assert_eq!(inner.scopes(), &[ScopeId(9)]);
}

#[test]
fn a_syntax_value_owns_its_tree() {
    // `value::build::syntax` copies the tree into the value's own region, so
    // the value stays readable after the arena it was read from is gone.
    //
    // The counter-factual: sharing the source arena's child slices and string
    // payloads would leave the value pointing into freed pages the moment the
    // reader's heap dropped — the `with-traits` hazard `RegionSlice`'s module
    // docs describe, in syntax form.
    let mut store = crate::value::fiberheap::FiberHeap::new();
    let region = store.new_runtime_region();

    let wrapped = {
        let mut scratch = SyntaxHeap::new();
        let form = crate::reader::read_syntax(scratch.arena(), "(alpha beta)", "<scratch>")
            .expect("parses");
        let value = crate::value::build::syntax(&mut store, form, region);
        drop(scratch);
        value
    };

    let embedded = wrapped.as_syntax().expect("a syntax value");
    let SyntaxKind::List(items) = embedded.kind else {
        panic!("expected a list");
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_symbol(), Some("alpha"));
    assert_eq!(items[1].as_symbol(), Some("beta"));
    assert_eq!(
        owner_of(&arena_of(&mut store, region), items.as_ptr() as *const u8),
        region.get(),
        "the value's tree must live in the value's own region"
    );
}

/// An arena naming `region` on `store`, for the ownership assertion above.
fn arena_of(
    store: &mut crate::value::fiberheap::FiberHeap,
    region: crate::hir::region::RuntimeRegion,
) -> SyntaxArena {
    SyntaxArena::new(store, region)
}
