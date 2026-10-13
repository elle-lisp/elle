// audited: 2026-10-20
// `--boot-image=` at the command line: where the default stores, which flags
// turn it off, and which command never reads it.
//
// docs/impl/image/boot.md
// docs/config.md
//
// Each start is a process of its own, because the policy comes from the
// process configuration, which a test binary sets once for every test in it.

use std::path::Path;
use std::process::{Command, Output};

/// Run `elle` with `args`, failing the test if the start fails.
fn elle(args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(args)
        .output()
        .expect("run elle");
    assert!(
        out.status.success(),
        "`elle {args:?}` failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// Every `.image` file in `dir`'s `boot` directory.
fn images(dir: &Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir.join("boot"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "image"))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a `--trace=boot` start's stderr reports a hydration.
fn hydrated(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .any(|l| l.starts_with("[trace:boot]") && l.contains("image-hydrate"))
}

// § "The warm cache is on by default": a start that names no policy stores an
// image under the `--cache=` directory, and the next start hydrates it. The
// counter-factual is the opt-in policy, under which neither start touches the
// directory and every start compiles the three sources.
#[test]
fn a_start_with_no_policy_stores_an_image_the_next_start_hydrates() {
    let dir = crate::common::ScratchDir::new("boot-default");
    let cache = format!("--cache={}", dir.path().display());
    elle(&[&cache, "-e", "(+ 1 2)"]);
    let stored = images(dir.path());
    assert_eq!(
        stored.len(),
        1,
        "the first start stored no image: {stored:?}"
    );

    let next = elle(&[&cache, "--trace=boot", "-e", "(+ 1 2)"]);
    assert!(
        hydrated(&next),
        "the second start did not hydrate the image the first stored: {}",
        String::from_utf8_lossy(&next.stderr)
    );
}

// § "The warm cache is on by default": `--boot-image=off` neither reads the
// image a directory holds nor stores one in a directory that holds none.
#[test]
fn boot_image_off_reads_and_writes_no_image() {
    let dir = crate::common::ScratchDir::new("boot-off");
    let cache = format!("--cache={}", dir.path().display());
    elle(&[&cache, "--boot-image=off", "-e", "(+ 1 2)"]);
    assert!(
        !dir.join("boot").exists(),
        "a start with the warm cache off made its directory"
    );

    elle(&[&cache, "--boot-image=on", "-e", "(+ 1 2)"]);
    assert_eq!(images(dir.path()).len(), 1, "no image to ignore");
    let off = elle(&[&cache, "--boot-image=off", "--trace=boot", "-e", "(+ 1 2)"]);
    assert!(
        !hydrated(&off),
        "a start with the warm cache off hydrated an image"
    );
}

// § "Producing one": `elle image dump-boot` boots from source, so the
// artifact it writes is the one the three sources produce. A runtime that
// boots under the default policy stores into the cache when it misses, so a
// cache left empty is what shows that the dump's runtime never read it.
//
// `ELLE_CACHE` rather than `--cache=`, because the subcommand takes no flag
// of elle's own: it starts from the default configuration.
#[test]
fn dump_boot_writes_nothing_into_the_cache() {
    let dir = crate::common::ScratchDir::new("boot-dump-boot");
    let out_file = dir.join("boot.image");
    let cache = dir.join("cache");
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(["image", "dump-boot"])
        .arg(&out_file)
        .env("ELLE_CACHE", &cache)
        .output()
        .expect("run elle image dump-boot");
    assert!(
        out.status.success(),
        "dump-boot failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out_file.exists(), "dump-boot wrote no image");
    assert!(
        !cache.join("boot").exists(),
        "dump-boot's runtime used the warm cache"
    );
}
