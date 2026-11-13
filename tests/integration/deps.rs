// audited: 2026-11-13
// Every committed lockfile holds one Cranelift, one regalloc2 and each advised
// crate at its floor, and each variant's Cranelift is the root's.
//
// docs/impl/cranelift.md
// docs/analysis/advisories.md
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

/// A crate an advisory named, and the oldest release that carries every fix
/// the audit has named for it.
struct Floor {
    crate_name: &'static str,
    version: (u64, u64, u64),
    advisories: &'static str,
}

const fn floor(
    crate_name: &'static str,
    version: (u64, u64, u64),
    advisories: &'static str,
) -> Floor {
    Floor {
        crate_name,
        version,
        advisories,
    }
}

/// One row per crate `cargo audit` has named with a fixed release. When the
/// audit names a newer fix, raise the row in the same change as the lock.
const FLOORS: [Floor; 6] = [
    floor("wasmtime", (49, 0, 4), "RUSTSEC-2026-0378, -0379"),
    floor("zstd", (0, 13, 4), "RUSTSEC-2026-0391"),
    floor("zstd-safe", (7, 4, 0), "RUSTSEC-2026-0395 to -0406"),
    floor("zstd-sys", (2, 1, 2), "RUSTSEC-2026-0410"),
    floor("rustix", (1, 1, 6), "RUSTSEC-2026-0486"),
    floor("clipboard-win", (5, 4, 2), "5.4.0 and 5.4.1 are yanked"),
];

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
fn every_advised_crate_resolves_at_or_above_its_floor() {
    // A manifest range admits every release at or above its floor, so the
    // lock decides which one builds. A `cargo update` that goes backwards, or
    // a lock that never moved, leaves the advisory open, and nothing but the
    // audit job sees it. This test fails on the developer's machine first.
    // `cargo audit` reads the root's Cargo.lock alone, so for a variant's
    // lockfile — elle-wasm is the binary that links wasmtime — nothing else
    // sees it at all.
    let locks: Vec<_> = LOCKFILES
        .iter()
        .map(|path| (path, locked_versions(path)))
        .collect();
    let mut below = Vec::new();
    for Floor {
        crate_name,
        version: minimum,
        advisories,
    } in &FLOORS
    {
        let mut held = false;
        for (path, versions) in &locks {
            for version in versions.get(*crate_name).into_iter().flatten() {
                held = true;
                if parse_semver(version) < *minimum {
                    let (major, minor, patch) = minimum;
                    below.push(format!(
                        "{path}: {crate_name} {version} is below {major}.{minor}.{patch} ({advisories})"
                    ));
                }
            }
        }
        // A floor no lockfile holds guards nothing, and reads as a fix still
        // owed. Delete its row.
        assert!(
            held,
            "no committed lockfile holds {crate_name}; delete its floor"
        );
    }
    assert!(
        below.is_empty(),
        "crates below the release that fixed their advisories; run `cargo update -p` \
         on each in the lockfile's package (docs/analysis/advisories.md):\n{}",
        below.join("\n")
    );
}
