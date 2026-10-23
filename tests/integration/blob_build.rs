// audited: 2026-10-23
// The release build that embeds the boot image: two stages, the variable each
// release build passes, its checks, the target that builds it, and the builds
// without one.
//
// docs/impl/image/boot.md
// docs/analysis/ci.md
//
// Every assertion reads `make --dry-run`. The recipe is shell text nothing
// compiles, and a release build that embeds nothing still runs every suite
// green: each start then compiles the library or reads the warm cache.

use crate::common::{make_dry_run, make_dry_run_with};

/// The variable that names the blob to the build script.
const VAR: &str = "ELLE_BOOT_BLOB=";

/// What stage 1 passes cargo, so it never shares a library build with stage 2.
const STAGE_ONE: &str = "--target-dir target/stage1";

/// The file stage 1's dump writes and stage 2 embeds.
const BLOB: &str = "target/stage1/boot.image";

/// What `make TARGET` runs for a release build, or for a debug one.
fn recipe(target: &str, release: bool) -> String {
    let profile = if release {
        "CARGO_PROFILE=--release"
    } else {
        "CARGO_PROFILE="
    };
    make_dry_run_with(target, &[profile])
        .unwrap_or_else(|| panic!("`make --dry-run {target} {profile}` failed"))
}

/// Each line of `recipe` that runs cargo, with its byte offset in the recipe.
fn cargo_lines(recipe: &str) -> Vec<(usize, &str)> {
    let mut at = 0;
    let mut out = Vec::new();
    for line in recipe.split_inclusive('\n') {
        if line.contains("cargo build") || line.contains("cargo run") {
            out.push((at, line.trim_end()));
        }
        at += line.len();
    }
    out
}

/// The byte offset of `needle` in `recipe` at or after `from`.
fn find_after(recipe: &str, needle: &str, from: usize) -> Option<usize> {
    recipe[from..].find(needle).map(|i| i + from)
}

/// Stage 2's cargo line in a release `make elle`: it builds `elle` with the
/// variable and in the shared target directory.
fn stage_two(recipe: &str) -> (usize, &str) {
    cargo_lines(recipe)
        .into_iter()
        .find(|(_, l)| l.contains(VAR) && l.contains("-p elle") && !l.contains(STAGE_ONE))
        .unwrap_or_else(|| panic!("a release `make elle` builds no `elle` with {VAR}:\n{recipe}"))
}

// Stage 1 builds in a target directory of its own, its dump writes the blob,
// and stage 2 builds `elle` with the variable naming that file.
//
// The counter-factual for the directory is one shared target directory, where
// every `make elle` builds the library without the blob and then with it.
#[test]
fn a_release_make_elle_builds_stage_one_apart_and_embeds_its_dump() {
    let recipe = recipe("elle", true);
    let (one, line) = cargo_lines(&recipe)
        .into_iter()
        .find(|(_, l)| l.contains(STAGE_ONE))
        .unwrap_or_else(|| panic!("a release `make elle` has no stage 1:\n{recipe}"));
    assert!(
        line.contains("--release") && line.contains("-p elle"),
        "stage 1 is not a release build of elle: {line}"
    );
    assert!(
        !line.contains(VAR),
        "stage 1 builds with the blob it is about to write: {line}"
    );
    let dump = find_after(&recipe, "target/stage1/release/elle image dump-boot", one)
        .unwrap_or_else(|| panic!("stage 1's binary dumps no boot image:\n{recipe}"));

    let (two, line) = stage_two(&recipe);
    assert!(
        two > dump,
        "stage 2 builds before stage 1 dumps its blob:\n{recipe}"
    );
    let value = line
        .split(VAR)
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_default();
    assert!(
        value.ends_with(BLOB) && value.starts_with('/'),
        "stage 2 names `{value}` rather than the absolute path of {BLOB}: {line}"
    );
}

// Stage 2's own dump writes the blob it carries again, and a start of stage 2
// maps the blob and hands it to a worker. The counter-factual is a build that
// embeds without checking: a blob from another compile, or one no start finds
// in its own file, still boots, and every suite passes on the copy or on the
// warm cache.
#[test]
fn a_release_make_elle_checks_the_blob_it_embedded() {
    let recipe = recipe("elle", true);
    let (two, _) = stage_two(&recipe);
    let dump = find_after(&recipe, "target/release/elle image dump-boot", two)
        .unwrap_or_else(|| panic!("stage 2 never dumps the blob again:\n{recipe}"));
    assert!(
        find_after(&recipe, "cmp", dump).is_some(),
        "stage 2's dump is never compared with the blob:\n{recipe}"
    );
    let tail = &recipe[two..];
    for needle in ["--trace=boot", "blob-map", "image-hydrate", "sys/spawn"] {
        assert!(
            tail.contains(needle),
            "stage 2's start never reads `{needle}`, so nothing proves it mapped \
             the blob and its worker hydrated it:\n{recipe}"
        );
    }
}

// Every release cargo call that links the default-feature library passes the
// variable. A call without it rebuilds that library without the blob, and the
// next `make elle` rebuilds it with the blob again.
#[test]
fn every_release_build_of_the_default_library_embeds_the_blob() {
    let rig = recipe("elle-rig", true);
    let (at, line) = cargo_lines(&rig)
        .into_iter()
        .find(|(_, l)| l.contains("-p elle-rig"))
        .unwrap_or_else(|| panic!("a release `make elle-rig` builds no rig:\n{rig}"));
    assert!(line.contains(VAR), "the release rig embeds no blob: {line}");
    assert!(
        rig[..at].contains("image dump-boot"),
        "the release rig builds before any stage writes the blob:\n{rig}"
    );

    let embedding = recipe("embedding", true);
    for (_, line) in cargo_lines(&embedding) {
        if line.contains("-p elle-embed") {
            assert!(
                line.contains(VAR),
                "the release embedding demo links the library without the blob: {line}"
            );
        }
    }

    let noffi = recipe("elle-noffi", true);
    let lines = cargo_lines(&noffi);
    let (bare, line) = lines
        .iter()
        .find(|(_, l)| l.contains("--no-default-features"))
        .unwrap_or_else(|| panic!("`make elle-noffi` builds no featureless elle:\n{noffi}"));
    assert!(
        !line.contains(VAR),
        "the featureless build embeds the default build's blob: {line}"
    );
    assert!(
        lines.iter().any(|(at, l)| at > bare
            && l.contains(VAR)
            && l.contains("-p elle")
            && !l.contains("--no-default-features")
            && !l.contains(STAGE_ONE)),
        "`make elle-noffi` rebuilds the default elle without the blob:\n{noffi}"
    );
}

// A debug `make elle` builds once and embeds nothing.
#[test]
fn a_debug_make_elle_embeds_nothing() {
    let recipe = recipe("elle", false);
    assert!(
        !recipe.contains(VAR) && !recipe.contains("target/stage1"),
        "a debug `make elle` builds the blob:\n{recipe}"
    );
}

// A variant build passes no variable. A blob is what one build dumped, and the
// fingerprint does not record the cargo features, so a variant carrying the
// default build's blob would hydrate another build's image.
#[test]
fn no_variant_build_embeds_the_blob() {
    for target in ["elle-nojit", "elle-pool", "elle-mlir", "elle-wasm"] {
        let recipe = recipe(target, true);
        assert!(
            !recipe.contains(VAR),
            "the release `make {target}` embeds the default build's blob:\n{recipe}"
        );
    }
}

// `make release` is the release `make elle` with nothing on the command line.
// The counter-factual is the default it replaces: outside CI a plain
// `make elle` builds a debug binary that embeds nothing.
#[test]
fn make_release_builds_the_release_elle_with_its_blob() {
    let recipe =
        make_dry_run("release").unwrap_or_else(|| panic!("`make --dry-run release` failed"));
    assert!(
        cargo_lines(&recipe)
            .iter()
            .any(|(_, l)| l.contains(STAGE_ONE) && l.contains("--release")),
        "`make release` runs no release stage 1:\n{recipe}"
    );
    let (_, line) = stage_two(&recipe);
    assert!(
        line.contains("--release"),
        "`make release` builds a stage 2 that is no release build: {line}"
    );
}
