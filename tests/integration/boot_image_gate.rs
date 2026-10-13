// audited: 2026-10-20
// The boot-image gate: the runner's flag, the target that runs the language
// suite from an image, and the bytecode parity it checks first.
//
// docs/impl/image/boot.md
// docs/analysis/ci.md
//
// Every part fails quietly, which is why each is pinned here. `elle test`
// partitions its argv: a flag it does not know is not refused, it is handed to
// the runner as a file path. And the gate target is shell text nothing
// compiles, so a target that drops the flag runs the language suite on an
// ordinary source boot and reports exactly the green a hydrated run reports.

use crate::common::{make_dry_run, make_var};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// The gate target's name, spelled once.
const GATE: &str = "smoke-boot-image";

/// What `make` says the gate target will run, without running it.
///
/// Asking `make` rather than reading the Makefile is what makes this a test of
/// the recipe as it expands: the directory, the binary and the runner flags all
/// arrive through variables, and a parser that resolved them itself would be a
/// second `make` that can disagree with the first.
fn gate_recipe() -> String {
    make_dry_run(GATE).unwrap_or_else(|| panic!("`make --dry-run {GATE}` failed; no such target"))
}

/// Every `--boot-image=` value the gate's recipe names. A value that closes a
/// quoted argument, such as `--isolate '--boot-image=DIR'`, ends before the quote.
fn boot_image_values(recipe: &str) -> Vec<String> {
    recipe
        .split("--boot-image=")
        .skip(1)
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .unwrap_or_default()
                .trim_end_matches(['\'', '"'])
                .to_string()
        })
        .collect()
}

// The flag has to survive `elle test`'s argv partition. The runner is an Elle
// program handed everything the partition does not claim, so an unclaimed
// `--boot-image=DIR` becomes a file path: the run then fails on a file that
// is not there, and nothing ever hydrates. The counter-factual is the whole
// gate — point the language suite at an image through a flag the runner
// swallows, and every batch boots from source under a target that says
// otherwise.
#[test]
fn the_runner_boots_from_the_image_its_flag_names() {
    let dir = crate::common::ScratchDir::new("boot-image-runner");
    let flag = format!("--boot-image={}", dir.path().display());
    let file = dir.join("arithmetic.lisp");
    std::fs::write(&file, "(assert (= (+ 1 2) 3) \"the runner ran a file\")\n")
        .expect("write the file the runner runs");

    // Fill the cache: this start meets an empty directory, so it compiles the
    // three sources and stores what the run below has to hydrate.
    let stored = Command::new(elle_binary())
        .arg(&flag)
        .arg(&file)
        .output()
        .expect("run elle");
    assert!(
        stored.status.success(),
        "the storing run failed: {}",
        String::from_utf8_lossy(&stored.stderr)
    );

    let out = Command::new(elle_binary())
        .arg("test")
        .arg(&flag)
        .arg("--trace=boot")
        .args(["--timeout", "60000"])
        .arg("--db")
        .arg(dir.join("session.db"))
        .arg(&file)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle test");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "`elle test {flag}` did not pass the file it was given: {stderr}"
    );
    assert!(
        stderr
            .lines()
            .any(|l| l.starts_with("[trace:boot]") && l.contains("image-hydrate")),
        "`elle test {flag}` booted without hydrating the image, so the language suite \
         under the gate would run on a source boot: {stderr}"
    );
}

// A gate that greens whatever it is given gates nothing. The target names one
// directory, fills it, and hands the same one to every suite batch; a recipe
// that names two directories fills one and reads the other, which is a source
// boot with extra steps.
#[test]
fn the_gate_target_points_the_language_suite_at_one_image_directory() {
    let recipe = gate_recipe();
    let values = boot_image_values(&recipe);
    assert!(
        !values.is_empty(),
        "`make {GATE}` names no `--boot-image=`, so it runs the language suite on a \
         source boot:\n{recipe}"
    );
    let first = &values[0];
    assert!(
        values.iter().all(|v| v == first),
        "`make {GATE}` names more than one boot-image directory, so the one it \
         fills is not the one the language suite reads: {values:?}"
    );
    assert!(
        first.contains('/'),
        "`make {GATE}` points the language suite at `{first}` rather than at a \
         directory of its own"
    );

    // Every suite batch goes through the runner, which hands each file's child
    // its flags, and the child is the process whose boot the flag decides. A
    // recipe carrying the flag anywhere else proves nothing about the run that
    // matters.
    let elle = make_var("ELLE", &[]).expect("`make print-ELLE` did not run");
    let runner = format!("{elle} test");
    let batches: Vec<&str> = recipe
        .lines()
        .filter(|line| line.contains(&runner))
        .collect();
    assert!(
        !batches.is_empty(),
        "`make {GATE}` never runs `{runner}`, so it does not run the language suite:\n{recipe}"
    );
    for line in batches {
        assert!(
            line.contains("--boot-image="),
            "a suite batch in `make {GATE}` runs without the flag, so those \
             files boot from source: {line}"
        );
    }
}

// The trap `make check-wasm` already guards, in the boot image's shape: a
// binary that ignores `--boot-image=` accepts it and boots from source, and an
// image every start refuses is replaced and refused again. Either way the
// suite passes and the gate reports a hydration that never happened. So the
// target asks for the boot mark and fails without it.
//
// The counter-factual: drop the proof, break the warm cache's hydration, and
// this gate stays green while every other suite job also stays green — no
// other job boots from the warm cache.
#[test]
fn the_gate_target_proves_the_image_hydrated_before_it_runs_the_language_suite() {
    let recipe = gate_recipe();
    assert!(
        recipe.contains("--trace=boot"),
        "`make {GATE}` runs no `--trace=boot` start, so it cannot tell a \
         hydrated boot from a source boot:\n{recipe}"
    );
    assert!(
        recipe.contains("image-hydrate"),
        "`make {GATE}` never reads the `image-hydrate` mark, so a binary that \
         ignored `--boot-image=` would pass it:\n{recipe}"
    );
}

// ── The compile half of parity ────────────────────────────────────────

/// A program that reads the image's body fragments and a qualified key: the
/// calls to `map` and `push` graft the stdlib bodies they name
/// (docs/impl/graft.md), and `m:quokka` names a key no token in the file spells.
const PARITY_FIXTURE: &str = "(defn grow [xs] (map inc xs))\n\
                              (defn add-three [] (push [1 2] 3))\n\
                              (defn quokka-of [m] m:quokka)\n\
                              (quokka-of {})\n";

/// The first line where `a` and `b` part, numbered from one, or `None` when
/// they agree. A whole dump is thousands of lines, so a failure names the
/// place rather than printing both.
fn first_difference(a: &str, b: &str) -> Option<(usize, String, String)> {
    let mut left = a.lines();
    let mut right = b.lines();
    let mut n = 0;
    loop {
        n += 1;
        match (left.next(), right.next()) {
            (None, None) => return None,
            (l, r) if l == r => continue,
            (l, r) => {
                return Some((
                    n,
                    l.unwrap_or("<end>").to_string(),
                    r.unwrap_or("<end>").to_string(),
                ))
            }
        }
    }
}

// An image boot compiles a program to the bytecode a source boot does, each
// boot in a process of its own: the static-region counter is process-global,
// so two boots in one process would share it (docs/impl/image/boot.md).
//
// The counter-factual is a hydration that leaves that counter where a fresh
// process starts it. The program compiles, fuses and runs as before, and every
// region slot in its bytecode is lower than the source boot's — which only a
// comparison of the bytecode itself can see.
#[test]
fn an_image_boot_compiles_a_program_to_the_bytecode_a_source_boot_does() {
    let dir = crate::common::ScratchDir::new("boot-parity");
    let file = dir.join("parity.lisp");
    std::fs::write(&file, PARITY_FIXTURE).expect("write the fixture");
    let image = format!("--boot-image={}", dir.join("boot").display());

    let stored = Command::new(elle_binary())
        .args(["--cache=", &image, "-e", "(+ 1 2)"])
        .output()
        .expect("run elle");
    assert!(
        stored.status.success(),
        "the storing start failed: {}",
        String::from_utf8_lossy(&stored.stderr)
    );

    let dump = |flags: &[&str]| {
        let out = Command::new(elle_binary())
            .arg("--cache=")
            .args(flags)
            .arg("--dump=bytecode")
            .arg(&file)
            .output()
            .expect("run elle");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            out.status.success(),
            "`elle {flags:?} --dump=bytecode` failed: {stderr}"
        );
        (String::from_utf8_lossy(&out.stdout).into_owned(), stderr)
    };
    let (source, _) = dump(&[]);
    let (hydrated, trace) = dump(&[&image, "--trace=boot"]);

    assert!(
        trace
            .lines()
            .any(|l| l.starts_with("[trace:boot]") && l.contains("image-hydrate")),
        "the image dump booted from source, so the comparison is between two \
         source boots: {trace}"
    );
    assert!(
        source.contains("region="),
        "the source dump names no region slot, so a counter that never moved \
         would pass:\n{source}"
    );
    if let Some((line, want, got)) = first_difference(&source, &hydrated) {
        panic!(
            "an image boot compiles the fixture to other bytecode, first at line \
             {line}:\n  source boot: {want}\n  image boot:  {got}"
        );
    }
}

// `make smoke-boot-image` runs the parity check before the language suite. A
// corpus that answers alike under an image boot can still have compiled to
// other bytecode, and the suite cannot see that; the dumps can. The recipe is
// read as `make` expands it, so a prerequisite dropped from the target fails
// here rather than leaving a green job that compares nothing.
#[test]
fn the_gate_target_compares_bytecode_before_it_runs_the_language_suite() {
    let recipe = gate_recipe();
    let elle = make_var("ELLE", &[]).expect("`make print-ELLE` did not run");
    let runner = format!("{elle} test");
    let parity = recipe
        .find("scripts/bytecode-golden parity")
        .unwrap_or_else(|| panic!("`make {GATE}` never runs the bytecode parity check:\n{recipe}"));
    let suite = recipe
        .find(&runner)
        .unwrap_or_else(|| panic!("`make {GATE}` never runs `{runner}`:\n{recipe}"));
    assert!(
        parity < suite,
        "`make {GATE}` runs the language suite before the parity check:\n{recipe}"
    );
}

// The parity check compares two sets of dumps, and two source boots always
// agree. So it refuses to dump anything until a start proves it hydrated the
// image it stored. The binary here drops every `--boot-image=` it is handed —
// the binary that accepts the flag and ignores it, which a check without the
// proof would pass on every file.
#[cfg(unix)]
#[test]
fn the_parity_check_refuses_to_compare_two_source_boots() {
    use std::os::unix::fs::PermissionsExt;

    let dir = crate::common::ScratchDir::new("boot-parity-refusal");
    let deaf = dir.join("deaf-elle");
    std::fs::write(
        &deaf,
        format!(
            "#!/bin/sh\n\
             for a do shift; case \"$a\" in --boot-image=*) ;; *) set -- \"$@\" \"$a\" ;; esac; done\n\
             exec '{}' \"$@\"\n",
            elle_binary()
        ),
    )
    .expect("write the wrapper");
    std::fs::set_permissions(&deaf, std::fs::Permissions::from_mode(0o755))
        .expect("make the wrapper executable");

    let out_dir = dir.join("parity");
    let out = Command::new(crate::common::repo_root().join("scripts/bytecode-golden"))
        .arg("parity")
        .arg(&deaf)
        .arg(&out_dir)
        .arg("1")
        .output()
        .expect("run the parity check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "the parity check passed over a binary that never hydrates: {stderr}"
    );
    assert!(
        stderr.contains("did not hydrate"),
        "the parity check failed without saying the image never hydrated: {stderr}"
    );
    assert!(
        !out_dir.join("source").exists(),
        "the parity check dumped files before it proved the image hydrated"
    );
}

/// The last `--boot-image=` value an argv names, which is the one `elle`
/// keeps, or `None` when it names none.
fn effective_boot_image(argv: &str) -> Option<&str> {
    argv.split_whitespace()
        .filter_map(|arg| arg.strip_prefix("--boot-image="))
        .next_back()
}

// The parity check's source boots turn the boot image off by name. A binary
// that carries a blob hydrates it under `--cache=` alone, so a source side
// that named no policy would compare the blob with the warm cache: two image
// boots, which agree on exactly the counter that parity exists to catch.
//
// The binary here records each argv and answers the way a hydrating `elle`
// does, so the check runs every dump in seconds. A debug `elle` carries no
// blob, so only the argv can show what a release binary would boot.
#[cfg(unix)]
#[test]
fn the_parity_checks_source_boots_turn_the_boot_image_off() {
    use std::os::unix::fs::PermissionsExt;

    let dir = crate::common::ScratchDir::new("boot-parity-argv");
    let log = dir.join("argv.log");
    let fake = dir.join("recording-elle");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\n\
             echo \"$*\" >> '{}'\n\
             case \" $* \" in *' --trace=boot '*) echo '[trace:boot] image-hydrate 0.1ms' >&2 ;; esac\n\
             case \" $* \" in *' --dump=bytecode '*) echo 'one dump' ;; esac\n\
             exit 0\n",
            log.display()
        ),
    )
    .expect("write the recording binary");
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755))
        .expect("make the recording binary executable");

    let out_dir = dir.join("parity");
    let out = Command::new(crate::common::repo_root().join("scripts/bytecode-golden"))
        .arg("parity")
        .arg(&fake)
        .arg(&out_dir)
        .arg("8")
        .output()
        .expect("run the parity check");
    assert!(
        out.status.success(),
        "the parity check failed over a binary that answers alike: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let image = format!("{}/boot", out_dir.display());
    let argvs = std::fs::read_to_string(&log).expect("read the recorded argvs");
    let dumps: Vec<&str> = argvs
        .lines()
        .filter(|argv| argv.contains("--dump=bytecode"))
        .collect();
    let files = crate::common::suite_files().len() + 1;
    assert_eq!(dumps.len(), 2 * files, "one dump per file under each boot");
    let off = dumps
        .iter()
        .filter(|argv| effective_boot_image(argv) == Some("off"))
        .count();
    let hydrated = dumps
        .iter()
        .filter(|argv| effective_boot_image(argv) == Some(image.as_str()))
        .count();
    assert_eq!(
        (off, hydrated),
        (files, files),
        "each file dumps once with the boot image off and once from the image; \
         a dump that names no policy boots whatever the binary carries:\n{}",
        dumps.iter().take(4).copied().collect::<Vec<_>>().join("\n")
    );
}
