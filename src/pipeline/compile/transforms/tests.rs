use super::*;

fn count_7(s: &Syntax, ints: &mut usize, floats: &mut usize) {
    match &s.kind {
        SyntaxKind::Int(7) => *ints += 1,
        SyntaxKind::Float(f) if *f == 7.0 => *floats += 1,
        _ => {}
    }
    s.as_list()
        .into_iter()
        .flatten()
        .for_each(|c| count_7(c, ints, floats));
}

/// `splice_includes` re-stringifies each top-level form through `Syntax`
/// Display and re-reads it. An integral-valued float literal must survive as
/// a float: a `Float(7.0)` that Display renders as `7` re-reads as `Int(7)`,
/// silently changing `(type-of 7.0)`'s answer from :float to :integer. This
/// is the source of the WASM full-module float-literal corruption (only user
/// code is spliced, so only user integral floats break; stdlib is verbatim).
/// The end-to-end pins live in `tests/wasm_smoke` (float arithmetic).
#[test]
fn splice_includes_preserves_integral_float() {
    let spliced = splice_includes("(type-of 7.0)", "<t>").unwrap();
    let forms = read_syntax_all_for(crate::syntax::thread_arena(), &spliced, "<t>").unwrap();
    let (mut ints, mut floats) = (0, 0);
    for f in &forms {
        count_7(f, &mut ints, &mut floats);
    }
    assert_eq!(
        (ints, floats),
        (0, 1),
        "splice_includes retyped the float 7.0 (spliced text: {spliced:?})"
    );
}
