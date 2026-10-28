// audited: 2026-10-28
// The demos that need nothing outside the tree still compile through the file front end.
//
// demos/logo/README.md
//
// Nothing runs a demo on a build, so a demo that stops compiling stays broken
// until somebody runs it by hand. A compile runs none of the program: the logo
// demo writes docs/logo.svg when it runs, and a compile leaves the tree alone.

use elle::compile_file;
use elle::runtime::Runtime;
use std::path::PathBuf;

/// Compile the demo at `path`, relative to the repository root, without
/// running it.
fn compile_demo(path: &str) -> Result<(), String> {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(path);
    let src = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut rt = Runtime::new();
    let (_vm, symbols, cctx) = rt.parts();
    compile_file(&src, symbols, cctx, path)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// The counter-factual is three compile errors, each hiding the next. The
// demo's `let*` forms nested each pair in brackets, a shape the language no
// longer reads. It called `math/round`, which no longer exists. With both
// repaired, the `*` in `taper` failed its operand proof, because the fold
// deleted the arms of the `each` in `fiber-path-data` that reach no array.
#[test]
fn the_logo_demo_compiles() {
    compile_demo("demos/logo/logo.lisp")
        .unwrap_or_else(|e| panic!("demos/logo/logo.lisp must compile: {e}"));
}
