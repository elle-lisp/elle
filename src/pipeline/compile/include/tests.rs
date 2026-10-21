// audited: 2026-10-21
//! Splicing an include re-reads the forms it renders, and the round trip keeps each literal's type.
//!
//! docs/include.md

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
/// a float: a `Float(7.0)` that Display rendered as `7` would re-read as
/// `Int(7)`, and `(type-of 7.0)` would answer :integer. Only user code is
/// spliced, so the WASM full-module build would compute with the wrong type
/// while the stdlib stayed correct. tests/wasm_smoke runs float arithmetic end
/// to end.
#[test]
fn splice_includes_preserves_integral_float() {
    let mut cctx = CompileCtx::new();
    let spliced = splice_includes("(type-of 7.0)", "<t>", &mut cctx).unwrap();
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
