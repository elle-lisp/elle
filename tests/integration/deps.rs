// audited: 2026-09-30
// Every committed lockfile holds one Cranelift, one regalloc2 and a wasmtime
// past its advisory, and each variant's holds the root's.
//
// docs/impl/wasm.md
// bins/overview.md
//
// These read each `Cargo.lock` directly. A duplicated crate is invisible to
// every other test in the suite — both copies compile, both work — so nothing
// else in the tree can catch the split. The variant packages under bins/ are
// workspaces of their own, so each resolves its own graph and keeps its own
// lockfile, and the root's pins reach them only through that file.

use std::collections::BTreeMap;

/// Every lockfile the repository commits: the root workspace's, then each
/// variant package's.
const LOCKFILES: [&str; 3] = ["Cargo.lock", "bins/wasm/Cargo.lock", "bins/mlir/Cargo.lock"];

/// Every `[[package]]` in the lockfile at `path`, as name -> the versions
/// resolved for it. A name with more than one entry is in the graph twice.
fn locked_versions(path: &str) -> BTreeMap<String, Vec<String>> {
    let lock = std::fs::read_to_string(crate::common::repo_root().join(path))
        .unwrap_or_else(|e| panic!("{path} is committed beside its manifest: {e}"));
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

/// The one version of `crate_name` in the lockfile at `path`.
fn single_version(path: &str, crate_name: &str, why: &str) -> String {
    let versions = locked_versions(path);
    let found = versions
        .get(crate_name)
        .unwrap_or_else(|| panic!("{crate_name} is not in {path}"));
    assert_eq!(
        found.len(),
        1,
        "{path}: {crate_name} resolves to {found:?}; expected exactly one version. {why}"
    );
    found[0].clone()
}

/// Assert that every lockfile holds one version of `crate_name`, and that each
/// variant's is the root's.
fn assert_one_version_everywhere(crate_name: &str, why: &str) {
    let root = single_version(LOCKFILES[0], crate_name, why);
    for path in &LOCKFILES[1..] {
        assert_eq!(
            single_version(path, crate_name, why),
            root,
            "{path} resolves {crate_name} to another version than the root's \
             Cargo.lock, so the variant compiles a code generator the default \
             build does not"
        );
    }
}

#[test]
fn the_graph_holds_one_cranelift_codegen() {
    // The JIT (`cranelift-jit`) and the WASM tier (`wasmtime`) both pull the
    // code generator. Two versions compile it twice and, because Cargo picks
    // one `regalloc2` for the whole graph, hand the default-build JIT a
    // register allocator chosen by an opt-in tier.
    assert_one_version_everywhere(
        "cranelift-codegen",
        "Raise the `cranelift-*` pins in Cargo.toml to match wasmtime's Cranelift.",
    );
}

#[test]
fn the_graph_holds_one_regalloc2() {
    // The allocator that assigns registers in JIT-compiled native code. Two
    // copies mean the tier a given build runs is decided by dependency
    // resolution rather than by the pin.
    assert_one_version_everywhere(
        "regalloc2",
        "It follows cranelift-codegen; a split here means the Cranelift pins diverged.",
    );
}

/// The oldest `wasmtime` that carries the fix for its newest published
/// advisory (RUSTSEC-2026-0316). When `cargo audit` names a newer fix, raise
/// this in the same change as the pin in Cargo.toml.
const WASMTIME_FLOOR: (u64, u64, u64) = (49, 0, 1);

fn parse_semver(version: &str) -> (u64, u64, u64) {
    let mut parts = version.split(['.', '-', '+']).map(|part| {
        part.parse::<u64>()
            .unwrap_or_else(|_| panic!("{version:?} is not a MAJOR.MINOR.PATCH version"))
    });
    let mut next = || {
        parts
            .next()
            .unwrap_or_else(|| panic!("{version:?} is not a MAJOR.MINOR.PATCH version"))
    };
    (next(), next(), next())
}

#[test]
fn wasmtime_resolves_at_or_above_the_advisory_floor() {
    // A manifest range such as `wasmtime = "49"` admits every 49.x, so the
    // lock decides which one builds. A `cargo update` that goes backwards, or
    // a lock that never moved, leaves the advisory open, and nothing but the
    // audit job sees it. This test fails on the developer's machine first.
    // `cargo audit` reads the root's Cargo.lock alone, so for a variant's
    // lockfile — elle-wasm is the binary that links wasmtime — nothing else
    // sees it at all. See docs/impl/wasm.md § "One Cranelift in the dependency
    // graph".
    let (major, minor, patch) = WASMTIME_FLOOR;
    for path in LOCKFILES {
        let versions = locked_versions(path);
        let found = versions
            .get("wasmtime")
            .unwrap_or_else(|| panic!("wasmtime is not in {path}"));
        for version in found {
            assert!(
                parse_semver(version) >= WASMTIME_FLOOR,
                "{path}: wasmtime {version} is below {major}.{minor}.{patch}, the release \
                 that fixed its newest advisory. Raise the pin in Cargo.toml and run \
                 `cargo update -p wasmtime` in the lockfile's package."
            );
        }
    }
}
