// audited: 2026-09-29
// The variant packages under bins/ build the WASM and MLIR binaries from the
// root's sources, outside its workspace.
//
// bins/overview.md
// rig/overview.md
//
// No build of the root reads these manifests: the root workspace excludes
// them, so `cargo build`, `cargo clippy` and `cargo test` never compile them. A
// manifest that drifts from the rig's features or the root's profiles still
// builds, and the variant it builds is the one that differs. So each is read
// against the file it copies.

use crate::common::repo_root;

/// Each variant: its directory under bins/, which is also its tier's feature.
const VARIANTS: [&str; 2] = ["wasm", "mlir"];

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn manifest(variant: &str) -> String {
    read(&format!("bins/{variant}/Cargo.toml"))
}

/// The lines of the section `header` opens, up to the next header, less blank
/// lines and comments.
fn section(text: &str, header: &str) -> Vec<String> {
    text.lines()
        .skip_while(|line| line.trim() != header)
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Every section header in `text` that starts with `prefix`.
fn headers(text: &str, prefix: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| line.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

/// The quoted items of a one-line list, `key = ["a", "b"]`.
fn items(line: &str) -> Vec<String> {
    let (_, list) = line.split_once('[').unwrap_or(("", ""));
    list.trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// The value of `key` in a section's lines.
fn value<'a>(lines: &'a [String], key: &str) -> Option<&'a String> {
    lines
        .iter()
        .find(|line| line.split('=').next().map(str::trim) == Some(key))
}

// The counter-factual: a member of the root workspace that turns `mlir` on
// turns it on for every package one root command builds, so `cargo test` and
// `cargo clippy --workspace` build the MLIR tier into the default build.
#[test]
fn the_root_workspace_excludes_the_variant_packages() {
    let root = read("Cargo.toml");
    let workspace = section(&root, "[workspace]");
    let exclude = value(&workspace, "exclude")
        .unwrap_or_else(|| panic!("the root workspace excludes nothing: {workspace:?}"));
    assert!(
        items(exclude).iter().any(|item| item == "bins"),
        "the root workspace does not exclude bins/: {exclude}"
    );
    assert!(
        !root.contains("\"bins/"),
        "the root manifest names a variant package as a member"
    );
    for variant in VARIANTS {
        assert!(
            manifest(variant)
                .lines()
                .any(|line| line.trim() == "[workspace]"),
            "bins/{variant}/Cargo.toml is not a workspace of its own, so cargo \
             looks for one above it and refuses the package"
        );
    }
}

// A variant is the default build's sources under another name. The
// counter-factual: a package with one binary builds `elle-mlir` and leaves the
// MLIR build's rig to be built as `elle-rig`, over the default build's.
#[test]
fn each_variant_package_builds_elle_and_the_rig_under_its_own_names() {
    for variant in VARIANTS {
        let text = manifest(variant);
        let bins: Vec<(Option<String>, Option<String>)> = text
            .split("[[bin]]")
            .skip(1)
            .map(|block| {
                let lines: Vec<String> = block
                    .lines()
                    .take_while(|line| !line.starts_with('['))
                    .map(|line| line.trim().to_string())
                    .collect();
                let field = |key: &str| {
                    value(&lines, key).map(|line| {
                        line.split_once('=')
                            .map(|(_, v)| v.trim().trim_matches('"').to_string())
                            .unwrap_or_default()
                    })
                };
                (field("name"), field("path"))
            })
            .collect();
        let want = vec![
            (
                Some(format!("elle-{variant}")),
                Some("../../src/main.rs".to_string()),
            ),
            (
                Some(format!("elle-rig-{variant}")),
                Some("../../rig/src/main.rs".to_string()),
            ),
        ];
        assert_eq!(
            bins, want,
            "bins/{variant}/Cargo.toml does not build elle and the rig under the \
             variant's names"
        );
    }
}

// The rig reads `cfg!(feature = …)` of the package that compiles it. The
// counter-factual: a package that declares only its own tier builds a rig that
// reads `jit` as off and refuses every sidecar that sets it.
#[test]
fn each_variant_package_declares_the_rigs_features_with_its_own_tier_on() {
    let rig = section(&read("rig/Cargo.toml"), "[features]");
    let rig_default = items(value(&rig, "default").expect("the rig names its default features"));
    for variant in VARIANTS {
        let features = section(&manifest(variant), "[features]");
        let others = |lines: &[String]| -> Vec<String> {
            let mut out: Vec<String> = lines
                .iter()
                .filter(|line| value(std::slice::from_ref(line), "default").is_none())
                .cloned()
                .collect();
            out.sort();
            out
        };
        assert_eq!(
            others(&features),
            others(&rig),
            "bins/{variant}/Cargo.toml declares other features than the rig's"
        );
        let mut want = rig_default.clone();
        want.push(variant.to_string());
        assert_eq!(
            items(value(&features, "default").expect("a default feature list")),
            want,
            "bins/{variant}/Cargo.toml does not build the default features and \
             its own tier"
        );
    }
}

// The counter-factual: a package that depends on `elle` with its default
// features on builds the JIT into a variant whose manifest turned it off.
#[test]
fn each_variant_package_depends_on_elle_as_the_rig_does() {
    let rig = section(&read("rig/Cargo.toml"), "[dependencies]");
    let want: Vec<String> = rig
        .iter()
        .map(|line| line.replace("path = \"..\"", "path = \"../..\""))
        .collect();
    for variant in VARIANTS {
        assert_eq!(
            section(&manifest(variant), "[dependencies]"),
            want,
            "bins/{variant}/Cargo.toml depends on other crates than the rig"
        );
    }
}

// A workspace's profiles come from its own root manifest. The counter-factual:
// a package with none builds its release binaries without LTO and keeps their
// symbols, so a variant's timings and sizes answer for a different build.
#[test]
fn each_variant_package_copies_the_roots_profiles() {
    let root = read("Cargo.toml");
    let profiles = headers(&root, "[profile.");
    assert!(
        profiles.len() > 1,
        "the root names {profiles:?}; the parse is broken, not the manifest"
    );
    for variant in VARIANTS {
        let text = manifest(variant);
        assert_eq!(
            headers(&text, "[profile."),
            profiles,
            "bins/{variant}/Cargo.toml names other profiles than the root"
        );
        for header in &profiles {
            assert_eq!(
                section(&text, header),
                section(&root, header),
                "bins/{variant}/Cargo.toml's {header} differs from the root's"
            );
        }
    }
}
