// Dependency-graph shape tests.
//
// These read `Cargo.lock` directly. A duplicated crate is invisible to every
// other test in the suite — both copies compile, both work — so nothing else
// in the tree can catch the split.

use std::collections::BTreeMap;

/// Every `[[package]]` in `Cargo.lock`, as name -> the versions resolved for
/// it. A name with more than one entry is in the graph twice.
fn locked_versions() -> BTreeMap<String, Vec<String>> {
    let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"))
        .expect("Cargo.lock is checked in at the workspace root");
    let mut versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut name: Option<String> = None;
    for line in lock.lines() {
        // Cargo.lock writes `name` then `version` inside each `[[package]]`,
        // and quotes both. Nothing else in the file is at column 0 with these
        // keys, so a scan needs no TOML parser.
        if let Some(value) = line.strip_prefix("name = ") {
            name = Some(value.trim_matches('"').to_string());
        } else if let Some(value) = line.strip_prefix("version = ") {
            if let Some(name) = name.take() {
                versions
                    .entry(name)
                    .or_default()
                    .push(value.trim_matches('"').to_string());
            }
        }
    }
    versions
}

fn assert_single_version(versions: &BTreeMap<String, Vec<String>>, crate_name: &str, why: &str) {
    let found = versions
        .get(crate_name)
        .unwrap_or_else(|| panic!("{crate_name} is not in Cargo.lock"));
    assert_eq!(
        found.len(),
        1,
        "{crate_name} resolves to {found:?}; expected exactly one version. {why}"
    );
}

#[test]
fn the_graph_holds_one_cranelift_codegen() {
    // The JIT (`cranelift-jit`) and the WASM tier (`wasmtime`) both pull the
    // code generator. Two versions compile it twice and, because Cargo picks
    // one `regalloc2` for the whole graph, hand the default-build JIT a
    // register allocator chosen by an opt-in tier.
    // See docs/impl/wasm.md § "One Cranelift in the dependency graph".
    assert_single_version(
        &locked_versions(),
        "cranelift-codegen",
        "Raise the `cranelift-*` pins in Cargo.toml to match wasmtime's Cranelift.",
    );
}

#[test]
fn the_graph_holds_one_regalloc2() {
    // The allocator that assigns registers in JIT-compiled native code. Two
    // copies mean the tier a given build runs is decided by dependency
    // resolution rather than by the pin.
    assert_single_version(
        &locked_versions(),
        "regalloc2",
        "It follows cranelift-codegen; a split here means the Cranelift pins diverged.",
    );
}
