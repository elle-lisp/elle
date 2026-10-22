// audited: 2026-10-20
// A worker boots as its parent did: from the image its parent holds, or by compiling.
//
// docs/impl/image/boot.md
// docs/threads.md
//
// Each start is a process of its own, run with `--cache=` pointed at a scratch
// directory, so the image and any stdlib cache land there. The policy comes
// from the process configuration, which a test binary sets once for all its
// tests.
//
// The proof that an instance hydrated is the `image-hydrate` mark a
// `--trace=boot` start prints, one per instance, workers included.

use std::path::Path;
use std::process::{Command, Output};

/// Run `elle` with `--cache=` at `cache` and `args`, failing the test if the
/// start fails.
fn elle(cache: &Path, args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg(format!("--cache={}", cache.display()))
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

/// How many instances a `--trace=boot` start hydrated.
fn hydrations(out: &Output) -> usize {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .filter(|l| l.starts_with("[trace:boot]") && l.contains("image-hydrate"))
        .count()
}

/// Whether a `--trace=boot` start stored an image.
fn stored(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .any(|l| l.starts_with("[trace:boot]") && l.contains("image-store"))
}

/// A cache directory holding the image a start stored.
fn warm(tag: &str) -> crate::common::ScratchDir {
    let dir = crate::common::ScratchDir::new(tag);
    elle(dir.path(), &["-e", "(+ 1 2)"]);
    dir
}

// The counter-factual for every mark below is a worker that compiles its own
// boot: it answers every call correctly and prints no mark.
#[test]
fn a_worker_of_an_image_booted_start_hydrates_the_image() {
    let dir = warm("worker-boot-heavy");
    let out = elle(
        dir.path(),
        &[
            "--trace=boot",
            "-e",
            "(println (sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3]))))))",
        ],
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "[2 3 4]",
        "the worker's stdlib answered wrongly"
    );
    assert_eq!(
        hydrations(&out),
        2,
        "the start and its worker each hydrate once: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// The light worker installs the core exports and the macros out of the image,
// and no stdlib export: `+` stays unbound in it under either boot. The
// counter-factual for the refusal is a light worker that installs the whole
// image, which resolves `+` and answers 3.
#[test]
fn a_light_worker_of_an_image_booted_start_hydrates_the_image_and_lacks_the_stdlib() {
    let dir = warm("worker-boot-light");
    let out = elle(
        dir.path(),
        &[
            "--trace=boot",
            "-e",
            "(println (get (protect (sys/join (sys/spawn-vm (fn [] (eval '(+ 1 2)))))) 1))",
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("undefined variable: +"),
        "the light worker resolved a stdlib name: {stdout}"
    );
    assert_eq!(
        hydrations(&out),
        2,
        "the start and its light worker each hydrate once: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// A worker hands on the image it hydrated, so a worker's worker maps it too.
// The inner join runs under `ev/run`, because a worker has no scheduler of its
// own (docs/threads.md).
#[test]
fn a_worker_that_a_worker_spawns_hydrates_the_image_too() {
    let dir = warm("worker-boot-nested");
    let out = elle(
        dir.path(),
        &[
            "--trace=boot",
            "-e",
            "(println (sys/join (sys/spawn (fn [] (ev/run (fn [] (sys/join (sys/spawn (fn [] 7)))))))))",
        ],
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "7");
    assert_eq!(
        hydrations(&out),
        3,
        "the start, its worker and the worker's worker each hydrate once: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// A start that compiles and stores hands its workers the file it stored.
#[test]
fn a_cold_starts_worker_hydrates_the_image_the_start_stored() {
    let dir = crate::common::ScratchDir::new("worker-boot-cold");
    let out = elle(
        dir.path(),
        &["--trace=boot", "-e", "(sys/join (sys/spawn (fn [] 1)))"],
    );
    assert!(stored(&out), "the cold start stored no image");
    assert_eq!(
        hydrations(&out),
        1,
        "the worker hydrates the image its parent stored: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// A worker never reads the process policy itself. A `--no-stdlib` start holds
// no image, so its worker compiles, though the policy names a directory that
// holds one. The counter-factual is a worker that resolves the policy on its
// own and hydrates. The start has no `sys/join`, which is the stdlib's, so it
// polls the handle with the primitive the join is built on.
#[test]
fn a_no_stdlib_starts_worker_reads_no_image() {
    let dir = warm("worker-boot-no-stdlib");
    let out = elle(
        dir.path(),
        &[
            "--no-stdlib",
            "--trace=boot",
            "-e",
            "(def h (sys/spawn (fn [] 1))) \
             (while (%not (%eq :ready (get (sys/thread-state h) 0))) nil)",
        ],
    );
    assert_eq!(
        hydrations(&out),
        0,
        "a worker of a start that holds no image hydrated one: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// A worker whose inherited image refuses reports the refusal and compiles. The
// parent hydrates the file, then the test overwrites the file's first bytes in
// place. Those bytes are the header, which a hydration reads and never maps,
// so the parent's own mapping is untouched, and every hydration after that
// refuses the descriptor. The counter-factual is a worker that compiles without
// trying the image: it answers too, and reports no refusal.
#[test]
fn a_worker_whose_inherited_image_refuses_compiles_and_answers() {
    use std::os::unix::fs::FileExt;

    let dir = warm("worker-boot-refused");
    let ready = dir.join("ready");
    let go = dir.join("go");
    let program = format!(
        "(file/write {ready:?} \"1\") \
         (while (not (file/exists? {go:?})) (ev/sleep 0.01)) \
         (println (sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3]))))))",
        ready = ready.display().to_string(),
        go = go.display().to_string(),
    );
    let child = Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg(format!("--cache={}", dir.path().display()))
        .args(["-e", &program])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run elle");

    // The program runs after its instance has booted, so `ready` existing
    // means the parent has hydrated the image.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !ready.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the start never reached its program"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let image = std::fs::read_dir(dir.join("boot"))
        .expect("the warm start made the image directory")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "image"))
        .expect("the warm start stored an image");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&image)
        .expect("open the image for writing")
        .write_all_at(&[0u8; 8], 0)
        .expect("overwrite the header in place");
    std::fs::write(&go, "1").expect("signal the program");

    let out = child.wait_with_output().expect("wait for elle");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the start failed: {stderr}");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "[2 3 4]",
        "the worker that refused its image answered wrongly"
    );
    assert!(
        stderr.lines().any(|l| l.starts_with("[boot-image]")),
        "the worker reported no refused image: {stderr}"
    );
}
