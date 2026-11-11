// audited: 2026-11-11
//! What the hoist leaves where it is, and the node ids it keeps.
//!
//! docs/impl/hoist.md
//!
//! The counter-factual for each: a pass that moved every literal it met would
//! share one mutable value across passes, build a raise's payload on every
//! entry, or reach into a function the loop only builds.

use super::*;

#[test]
fn a_mutable_string_literal_stays_and_the_string_it_thaws_moves() {
    let (hir, arena) = hoisted(&in_loop(r#"(f @"x")"#));
    assert_eq!(moved_strings(&hir), ["x"], "the thawed string moves");
    // `@"x"` is a call through the `%thaw` binding, not an intrinsic node.
    assert_eq!(
        calls_in_loops(&hir, &arena, "%thaw"),
        1,
        "the thaw stays in the loop"
    );
}

#[test]
fn a_mutable_collection_literal_stays_and_its_constants_move() {
    for (form, ctor) in [
        (r#"@["a" 1]"#, "@array"),
        (r#"@{:k "a"}"#, "@struct"),
        (r#"@|"a"|"#, "@set"),
        ("@b[1 2]", "@bytes"),
    ] {
        let (hir, arena) = hoisted(&in_loop(&format!("(f {form})")));
        assert_eq!(
            calls_in_loops(&hir, &arena, ctor),
            1,
            "{form}: the constructor call stays in the loop"
        );
        assert!(
            moved(&hir).iter().all(|m| callee(m, &arena).is_none()),
            "{form}: no constructor call moves"
        );
    }
    let (hir, _) = hoisted(&in_loop(r#"(f @["a" 1])"#));
    assert_eq!(moved_strings(&hir), ["a"], "an item that is a string moves");
}

#[test]
fn a_quoted_datum_with_a_mutable_part_stays() {
    for form in ["'@[1 2]", r#"'@"x""#, "'[1 @[2]]"] {
        let (hir, _) = hoisted(&in_loop(&format!("(f {form})")));
        assert!(moved(&hir).is_empty(), "{form}: nothing moves");
    }
}

#[test]
fn a_collection_literal_with_a_computed_item_stays_and_its_constants_move() {
    for (form, ctor) in [
        (r#"|"a" i|"#, "set"),
        (r#"["a" (g)]"#, "array"),
        (r#"{(g) "a"}"#, "struct"),
        (r#"["a" ;xs]"#, "array"),
    ] {
        let (hir, arena) = hoisted(&in_loop(&format!("(f {form})")));
        assert_eq!(
            calls_in_loops(&hir, &arena, ctor),
            1,
            "{form}: the literal stays in the loop"
        );
        assert_eq!(moved_strings(&hir), ["a"], "{form}: its string moves");
    }
}

#[test]
fn a_constant_in_a_fn_in_the_loop_stays() {
    let (hir, _) = hoisted(&in_loop(r#"(f (fn [] "x"))"#));
    assert!(moved(&hir).is_empty(), "nothing moves out of the fn");
    assert_eq!(strings_in_loops(&hir), ["x"]);
}

#[test]
fn a_constant_a_raise_reads_stays() {
    for form in [
        r#"(if (f) (emit :error "boom") nil)"#,
        r#"(if (f) (emit :error {:error :x :message "boom"}) nil)"#,
    ] {
        let (hir, _) = hoisted(&in_loop(form));
        assert!(moved(&hir).is_empty(), "{form}: nothing moves");
        assert_eq!(strings_in_loops(&hir), ["boom"], "{form}");
    }
}

#[test]
fn a_constant_a_yield_reads_moves() {
    let (hir, _) = hoisted(&in_loop(r#"(emit :yield "tick")"#));
    assert_eq!(
        moved_strings(&hir),
        ["tick"],
        "only a raise keeps its constants"
    );
}

#[test]
fn a_constant_outside_every_loop_stays() {
    let (hir, _) = hoisted(r#"(fn [] (f "x") (g |"y"|))"#);
    assert!(hoist_lets(&hir).is_empty(), "no loop, no let");
    assert_eq!(strings(&hir), ["x", "y"]);
}

#[test]
fn the_pass_keeps_every_node_id() {
    let source = in_loop(r#"(f "a" |"b"| '(1 2)) (g @"c" (fn [] "d"))"#);
    let (mut after, mut arena, _) = HirFixture::new().stage(Stage::Functional).build(&source);
    let mut before_ids = ids(&after);
    hoist_loop_constants(&mut after, &mut arena);
    let after_ids = ids(&after);
    let seen: std::collections::HashSet<_> = after_ids.iter().collect();
    assert_eq!(seen.len(), after_ids.len(), "no id appears twice");
    before_ids.retain(|id| !after_ids.contains(id));
    assert!(
        before_ids.is_empty(),
        "every node keeps its id, a moved constant's included: lost {before_ids:?}"
    );
    assert!(
        !moved(&after).is_empty(),
        "the source has constants to move"
    );
}
