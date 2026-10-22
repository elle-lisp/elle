// audited: 2026-10-22
// Every bench the manifest builds has a source file and a row in the bench
// crate's index, and every file and every row has its manifest entry.
//
// benches/AGENTS.md
//
// Nothing builds the benches on every change: no CI job runs `cargo bench`,
// and `cargo test` compiles none of them. A bench file with no manifest entry
// compiles under nothing and reports nothing, and an index row with no file
// sends a reader to nothing. This reads the three lists against each other.

use crate::common::repo_root;
use std::collections::BTreeSet;

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

/// The names the manifest's `[[bench]]` sections declare.
fn manifest_benches() -> BTreeSet<String> {
    read("Cargo.toml")
        .split("[[bench]]")
        .skip(1)
        .filter_map(|block| {
            block
                .lines()
                .take_while(|line| !line.starts_with('['))
                .find_map(|line| {
                    let (key, value) = line.split_once('=')?;
                    (key.trim() == "name").then(|| value.trim().trim_matches('"').to_string())
                })
        })
        .collect()
}

/// The `.rs` files at the top of benches/, by stem.
fn bench_files() -> BTreeSet<String> {
    std::fs::read_dir(repo_root().join("benches"))
        .expect("read benches/")
        .map(|entry| entry.expect("a benches/ entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "rs"))
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_string))
        .collect()
}

/// The files the index's table links as `[name.rs](name.rs)`.
fn indexed_benches() -> BTreeSet<String> {
    read("benches/AGENTS.md")
        .lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once("| [")?;
            let (name, _) = rest.split_once("](")?;
            name.strip_suffix(".rs").map(str::to_string)
        })
        .collect()
}

// The counter-factual: a bench file nothing in the manifest names is a bench
// `cargo bench` never builds, and a manifest entry with no file fails every
// `cargo bench` invocation, this file's included.
#[test]
fn every_bench_file_is_a_manifest_target_and_the_other_way_round() {
    let manifest = manifest_benches();
    assert!(
        manifest.len() >= 3,
        "the manifest names {manifest:?}; the parse is broken, not the manifest"
    );
    assert_eq!(
        bench_files(),
        manifest,
        "the files under benches/ and the manifest's [[bench]] names differ"
    );
}

// The counter-factual: an index row naming a file that does not exist sends a
// reader to nothing, and a file the index never names is a bench nobody
// finds. The index is the one list a reader opens first.
#[test]
fn the_bench_index_names_every_bench_and_nothing_else() {
    let indexed = indexed_benches();
    assert!(
        indexed.len() >= 3,
        "the index names {indexed:?}; the parse is broken, not the index"
    );
    assert_eq!(
        indexed,
        bench_files(),
        "the rows of benches/AGENTS.md and the files under benches/ differ"
    );
}
