// Every Elle source path a build or doc driver names must resolve.
//
// Both drivers below name their inputs as text, so a moved file leaves a
// reference that no compiler checks. Worse, both fail QUIETLY: the Makefile's
// `find` roots are swallowed by `2>/dev/null`, so a missing root drops its
// files out of the format gate and the gate still exits 0; the doc generator
// runs only on a push to main, so a stale `slurp` path turns main red after
// the merge that caused it. These tests are the cheap standing check that the
// text still points at the tree.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Directories whose `.lisp` files are not this repository's to format:
/// build output, git internals, and the `plugins` submodule (which runs its
/// own format gate from its own Makefile).
const UNOWNED: &[&str] = &["target", ".git", "plugins"];

/// Every `*.lisp` file under `dir`, recursively, skipping `UNOWNED`.
/// Paths come back relative to the repository root.
fn collect_lisp(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => panic!("cannot read {}: {}", dir.display(), e),
    };
    for entry in entries {
        let path = entry.expect("directory entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if !UNOWNED.contains(&name) {
                collect_lisp(&path, root, out);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("lisp") {
            out.push(path.strip_prefix(root).expect("under root").to_path_buf());
        }
    }
}

/// The roots the Makefile hands to `find` when it builds `LISP_FILES` — the
/// exact set of directories `make fmt` and `make fmt-check` walk.
fn makefile_find_roots() -> Vec<String> {
    let makefile = fs::read_to_string(repo_root().join("Makefile")).expect("read Makefile");
    let line = makefile
        .lines()
        .find(|l| l.starts_with("LISP_FILES :="))
        .expect("Makefile defines LISP_FILES");
    let (_, after_find) = line.split_once("find ").expect("LISP_FILES calls find");
    let (roots, _) = after_find.split_once(" -name").expect("find passes -name");
    let roots: Vec<String> = roots.split_whitespace().map(str::to_string).collect();
    assert!(!roots.is_empty(), "LISP_FILES names no find roots: {line}");
    roots
}

// The format gate is only as wide as its `find` roots, and a root that no
// longer exists costs nothing at the shell — `find` reports it on stderr and
// the Makefile discards that. So the files under it stop being formatted and
// nothing anywhere fails. Counter-factual: with `src/` absent from the roots,
// the five Elle sources under src/ (prelude, stdlib, core, test, the Lua
// prelude) went unformatted and unchecked, and `make fmt-check` stayed green.
#[test]
fn format_gate_covers_every_elle_source() {
    let root = repo_root();
    let roots = makefile_find_roots();

    for named in &roots {
        assert!(
            root.join(named).exists(),
            "Makefile LISP_FILES names `{named}`, which does not exist. \
             `find` reports this to the discarded stderr, so every .lisp file \
             under it silently leaves the format gate."
        );
    }

    let mut sources = Vec::new();
    collect_lisp(&root, &root, &mut sources);
    assert!(
        sources.len() > 100,
        "found only {} .lisp files; the walk is broken, not the gate",
        sources.len()
    );

    let uncovered: Vec<_> = sources
        .iter()
        .filter(|src| {
            !roots
                .iter()
                .any(|named| src.starts_with(named.trim_end_matches('/')))
        })
        .collect();
    assert!(
        uncovered.is_empty(),
        "these Elle sources are outside every Makefile LISP_FILES root, so \
         `make fmt-check` never sees them: {uncovered:?}"
    );
}

// The doc generator reads the Elle sources whose comments and defns become the
// API reference. It runs on a push to main and nowhere earlier, so a path that
// no longer resolves is a red main rather than a failed PR. Counter-factual:
// the generator slurped "prelude.lisp" and "stdlib.lisp" from the repo root
// after both moved under src/, and every gate before the merge passed.
#[test]
fn docgen_source_inputs_exist() {
    let root = repo_root();
    let generator = root.join("demos/docgen/generate.lisp");
    let text = fs::read_to_string(&generator).expect("read the doc generator");

    let src_dir = text
        .split_once("(def @src-dir \"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(dir, _)| dir.to_string())
        .expect("the generator defines src-dir");

    // Each `(path/join src-dir "NAME.lisp")` in the configuration block.
    let inputs: Vec<String> = text
        .split("(path/join src-dir \"")
        .skip(1)
        .filter_map(|rest| rest.split_once('"').map(|(name, _)| name.to_string()))
        .collect();

    // A rewrite that changes the shape must fail here rather than quietly
    // check nothing: the generator documents the prelude and the stdlib.
    assert!(
        inputs.len() >= 2,
        "found {} source inputs in {}; expected the prelude and the stdlib. \
         If the generator's configuration changed shape, teach this test the \
         new shape — do not let it pass by matching nothing.",
        inputs.len(),
        generator.display()
    );

    for name in &inputs {
        let path = root.join(&src_dir).join(name);
        assert!(
            path.exists(),
            "the doc generator reads {}, which does not exist",
            path.display()
        );
    }
}
