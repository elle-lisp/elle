// audited: 2026-10-21
// Bytecode parity: an image boot compiles to a source boot's bytecode, and the
// check never compares two source boots.
//
// docs/impl/image/boot.md
// docs/impl/image/pins.md

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

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

    // Every dump turns the boot image off first, so the source dump compiles
    // whatever the binary carries, and the image dump's flag overrides it.
    let dump = |flags: &[&str]| {
        let out = Command::new(elle_binary())
            .args(["--cache=", "--boot-image=off"])
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
