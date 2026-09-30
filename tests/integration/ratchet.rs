// audited: 2026-09-30
// A producer judges every reading against the ledger, a direct run is the
// whole gate for that producer, and a row belongs to a build.
//
// docs/ratchet.md
//
// The counter-factual: a pin is a literal in the producer and "shrink-only" is
// a comment beside it, so nothing fails when the pin is raised, a fix that
// reclaims more leaves a loose pin behind, and a subject nobody measures any
// more is a silence nobody can see. These drive a scratch producer against a
// scratch ledger and read the verdicts off its exit status and its lines.

use std::path::PathBuf;
use std::process::{Command, Output};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A scratch ledger directory holding one ledger for `producer`, and the
/// producer file itself.
struct Bench {
    dir: crate::common::ScratchDir,
    producer: PathBuf,
}

impl Bench {
    /// Write `rows` as the ledger of a producer whose body is `source`.
    fn new(tag: &str, rows: &str, source: &str) -> Bench {
        let dir = crate::common::ScratchDir::new(&format!("ratchet-{tag}"));
        let producer = dir.join("producer.lisp");
        std::fs::write(&producer, source).expect("write the producer");
        let ledger = dir.join("ledger");
        std::fs::create_dir_all(&ledger).expect("create the ledger dir");
        std::fs::write(
            ledger.join("producer.lisp"),
            format!(
                "(elle/epoch 13)\n(producer {})\n{rows}\n",
                json_string(producer.to_str().expect("utf-8 path"))
            ),
        )
        .expect("write the ledger");
        Bench { dir, producer }
    }

    fn ledger_dir(&self) -> PathBuf {
        self.dir.join("ledger")
    }

    /// Run the producer directly, as `elle PATH`, with the scratch ledger.
    fn run(&self) -> Output {
        Command::new(elle_binary())
            .arg(&self.producer)
            .current_dir(repo_root())
            .env("ELLE_LEDGER", self.ledger_dir())
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run the producer")
    }
}

fn json_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Both streams, so an assertion can name a verdict wherever it was printed.
fn text(out: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The `measure` lines a run printed, in order.
fn lines(out: &Output) -> Vec<String> {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.starts_with("measure "))
        .map(str::to_string)
        .collect()
}

/// `elle -e FORM`, from the repository root.
fn eval(form: &str) -> Output {
    Command::new(elle_binary())
        .args(["-e", form])
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e")
}

/// The instrument judging as the build it runs on.
const IMPORT: &str = "(def r ((import \"std/ratchet\")))\n";

/// The reference build's key: the default build on Linux x86_64. A row with
/// no `:build` belongs to it, and only there is a pin two-sided.
const REFERENCE: &str = "jit-uring-linux-x86_64";

/// The instrument judging as `build`, whatever box runs the test.
fn judging_as(build: &str) -> String {
    format!("(def r ((import \"std/ratchet\") :build \"{build}\"))\n")
}

/// A build no box is, so the instrument judges away from every row's home.
const ELSEWHERE: &str = "interp-pool-plan9-mips";

#[test]
fn a_reading_within_its_pin_passes_and_prints_its_line() {
    let b = Bench::new(
        "within",
        "[\"answer\" :count 42]",
        &format!("{IMPORT}(r:read \"answer\" :count 42)\n(r:report)\n"),
    );
    let out = b.run();
    assert!(
        out.status.success(),
        "a reading at its pin passes:\n{}",
        text(&out)
    );
    let printed = lines(&out);
    assert_eq!(printed.len(), 1, "one reading, one line:\n{}", text(&out));
    for want in [
        "\"subject\":\"answer\"",
        "\"axis\":\"count\"",
        "\"value\":42",
        "\"half\":0",
        "\"unit\":\"count\"",
        "\"bound\":42",
        "\"kind\":\"pin\"",
        "\"verdict\":\"ok\"",
    ] {
        assert!(
            printed[0].contains(want),
            "the line carries {want}:\n{}",
            printed[0]
        );
    }
}

#[test]
fn a_reading_past_its_pin_the_worse_way_is_a_regression() {
    let b = Bench::new(
        "worse",
        "[\"answer\" :count 41]",
        &format!("{IMPORT}(r:read \"answer\" :count 42)\n(r:report)\n"),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "42 against a pin of 41 fails:\n{}",
        text(&out)
    );
    let t = text(&out);
    assert!(
        t.contains("regression") && t.contains("answer"),
        "the failure names the verdict and the subject:\n{t}"
    );
}

#[test]
fn a_reading_past_its_pin_the_better_way_is_stale_at_home() {
    // The trap this pins: a pin left loose is a ratchet that slipped. A later
    // regression back to 43 would pass a pin of 43, so the loose pin fails
    // until it is moved. The row belongs to the reference build, and the
    // stale side is judged there alone, so the instrument is told to judge
    // as that build: on a macOS or an AArch64 box the same reading is ok.
    let b = Bench::new(
        "better",
        "[\"answer\" :count 43]",
        &format!("{}(r:read \"answer\" :count 42)\n(r:report)\n", judging_as(REFERENCE)),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "42 against a pin of 43 is stale:\n{}",
        text(&out)
    );
    let t = text(&out);
    assert!(
        t.contains("stale") && t.contains("answer"),
        "the failure says stale and names the subject:\n{t}"
    );
}

#[test]
fn a_better_reading_on_another_build_passes_the_pin() {
    // The counter-factual: the thread-pool and MLIR rigs run every producer
    // in CI, and a pin two-sided everywhere fails the build that reclaims
    // more. Away from the row's build the pin is what the ceilings were.
    let b = Bench::new(
        "away-better",
        "[\"answer\" :count 43]",
        &format!("{}(r:read \"answer\" :count 42)\n(r:report)\n", judging_as(ELSEWHERE)),
    );
    let out = b.run();
    assert!(
        out.status.success(),
        "42 against another build's pin of 43 passes:\n{}",
        text(&out)
    );
    let printed = lines(&out);
    assert!(
        printed[0].contains("\"bound\":43") && printed[0].contains("\"verdict\":\"ok\""),
        "the line names the pin it passed:\n{}",
        printed[0]
    );
}

#[test]
fn a_worse_reading_on_another_build_still_fails_the_pin() {
    let b = Bench::new(
        "away-worse",
        "[\"answer\" :count 41]",
        &format!("{}(r:read \"answer\" :count 42)\n(r:report)\n", judging_as(ELSEWHERE)),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "42 against another build's pin of 41 is a regression:\n{}",
        text(&out)
    );
    assert!(text(&out).contains("regression"), "and says so:\n{}", text(&out));
}

#[test]
fn a_build_row_pins_its_own_build_two_sided() {
    // The row with `:build` replaces the row with none on that build, and
    // there it is a pin in both directions.
    let rows = "[\"answer\" :count 43]\n[\"answer\" :count 41 :build \"interp-pool-plan9-mips\"]";
    let worse = Bench::new(
        "own-worse",
        rows,
        &format!("{}(r:read \"answer\" :count 42)\n(r:report)\n", judging_as(ELSEWHERE)),
    );
    let out = worse.run();
    assert!(
        !out.status.success(),
        "42 against the build's own pin of 41 fails:\n{}",
        text(&out)
    );
    assert!(
        lines(&out)[0].contains("\"bound\":41") && text(&out).contains("regression"),
        "against the build's row, not the default one:\n{}",
        text(&out)
    );

    let better = Bench::new(
        "own-better",
        rows,
        &format!("{}(r:read \"answer\" :count 40)\n(r:report)\n", judging_as(ELSEWHERE)),
    );
    let out = better.run();
    assert!(
        !out.status.success() && text(&out).contains("stale"),
        "40 against the build's own pin of 41 is stale:\n{}",
        text(&out)
    );
}

#[test]
fn a_row_for_another_build_is_no_row_here() {
    // A `:build` row applies on its build alone: elsewhere the reading is
    // unledgered, and the row is not missing.
    let b = Bench::new(
        "foreign",
        "[\"answer\" :count 42 :build \"interp-pool-plan9-mips\"]",
        &format!("{}(r:read \"answer\" :count 42)\n(r:report)\n", judging_as(REFERENCE)),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "another build's row is no row for this one:\n{}",
        text(&out)
    );
    let t = text(&out);
    assert!(
        t.contains("unledgered") && !t.contains("missing"),
        "the reading is unledgered, and the foreign row is not missing:\n{t}"
    );
}

#[test]
fn the_build_key_names_the_tier_the_backend_and_the_platform() {
    let out = eval("(def l ((import \"std/ratchet/ledger\"))) (println (l:running-build))");
    let key = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let parts: Vec<&str> = key.split('-').collect();
    assert_eq!(
        parts.len(),
        4,
        "tier-backend-os-arch, got {key:?}:\n{}",
        text(&out)
    );
    assert!(
        ["jit", "mlir", "wasm", "interp"].contains(&parts[0]),
        "the tier the build carries, got {key:?}"
    );
    assert!(
        ["uring", "pool"].contains(&parts[1]),
        "the I/O backend it runs, got {key:?}"
    );
    assert_eq!(parts[2], std::env::consts::OS, "the operating system, got {key:?}");
    assert_eq!(parts[3], std::env::consts::ARCH, "the architecture, got {key:?}");
}

/// The build these tests run on is the reference build exactly when cargo
/// built the default features on Linux x86_64.
#[cfg(all(
    target_os = "linux",
    target_arch = "x86_64",
    feature = "jit",
    feature = "uring",
    not(feature = "mlir"),
    not(feature = "wasm")
))]
#[test]
fn the_reference_build_is_the_default_build_on_linux_x86_64() {
    let out = eval("(def l ((import \"std/ratchet/ledger\"))) (println (l:running-build))");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        REFERENCE,
        "this box is the reference build:\n{}",
        text(&out)
    );
    let out = eval("(def l ((import \"std/ratchet/ledger\"))) (println l:reference-build)");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        REFERENCE,
        "and the ledger names it:\n{}",
        text(&out)
    );
}

#[test]
fn the_rewrite_moves_the_row_of_the_build_it_is_given() {
    // Two rows share a subject and an axis and differ by build; the rewrite
    // must move the one the reading met and leave the other byte for byte.
    let text_form = "(def t \"[\\\"answer\\\" :count 43]\\n[\\\"answer\\\" :count 41 :build \\\"interp-pool-plan9-mips\\\"]\\n\")";
    let out = eval(&format!(
        "(def rp ((import \"std/ratchet/repin\"))) {text_form} \
         (print (rp:move t \"answer\" :count \"40\" \"interp-pool-plan9-mips\")) \
         (print \"--\") \
         (print (rp:move t \"answer\" :count \"40\" nil))"
    ));
    let printed = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(
        printed,
        "[\"answer\" :count 43]\n[\"answer\" :count 40 :build \"interp-pool-plan9-mips\"]\n\
         --[\"answer\" :count 40]\n[\"answer\" :count 41 :build \"interp-pool-plan9-mips\"]\n",
        "each move touches its own build's row:\n{}",
        text(&out)
    );
}

#[test]
fn an_adopted_reading_away_from_home_is_a_build_row() {
    let out = eval(
        "(def rp ((import \"std/ratchet/repin\"))) \
         (println (rp:row-for {:subject \"extra\" :axis :count :value 7 :half 0} \"interp-pool-plan9-mips\")) \
         (println (rp:row-for {:subject \"extra\" :axis :count :value 7 :half 0} nil))",
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "[\"extra\" :count 7 :build \"interp-pool-plan9-mips\"]\n[\"extra\" :count 7]\n",
        "a reading adopted away from home names its build, and one at home does not:\n{}",
        text(&out)
    );
}

#[test]
fn a_larger_reading_is_the_better_side_under_better_higher() {
    let b = Bench::new(
        "rising",
        "[\"coverage\" :files 10 :better :higher]",
        &format!("{IMPORT}(r:read \"coverage\" :files 9)\n(r:report)\n"),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "9 against a rising pin of 10 fails:\n{}",
        text(&out)
    );
    let t = text(&out);
    assert!(
        t.contains("regression") && t.contains("coverage"),
        "and it is a regression, named:\n{t}"
    );
}

#[test]
fn the_judge_compares_the_whole_interval() {
    // A reading of 5 ± 3 reaches 8, so it overlaps a pin of 7: what the
    // instrument cannot resolve, the gate does not hold.
    let b = Bench::new(
        "span",
        "[\"wide\" :count 7]",
        &format!("{IMPORT}(r:read \"wide\" :count 5 :half 3)\n(r:report)\n"),
    );
    let out = b.run();
    assert!(
        out.status.success(),
        "an interval straddling the pin passes:\n{}",
        text(&out)
    );
}

#[test]
fn slack_widens_a_pin_on_both_sides() {
    let b = Bench::new(
        "noisy",
        "[\"noisy\" :ms 100 :slack 10]",
        &format!("{IMPORT}(r:read \"noisy\" :ms 108)\n(r:read \"noisy\" :ms 108)\n(r:report)\n"),
    );
    let out = b.run();
    assert!(
        out.status.success(),
        "108 sits inside 100 ± 10:\n{}",
        text(&out)
    );
}

#[test]
fn a_reading_with_no_row_is_unledgered() {
    let b = Bench::new(
        "norow",
        "[\"something else\" :count 1]",
        &format!(
            "{IMPORT}(r:read \"something else\" :count 1)\n(r:read \"answer\" :count 42)\n(r:report)\n"
        ),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "a reading nobody pinned fails:\n{}",
        text(&out)
    );
    let t = text(&out);
    assert!(
        t.contains("unledgered") && t.contains("answer"),
        "the failure names the unledgered subject:\n{t}"
    );
}

#[test]
fn a_row_the_producer_never_reads_is_missing() {
    // The counter-factual for elle-lisp/elle#1144: a deleted probe left no
    // trace, so coverage rotted into silence. The row outlives the probe and
    // fails until somebody moves the ledger.
    let b = Bench::new(
        "unread",
        "[\"answer\" :count 42]\n[\"never read\" :count 1]",
        &format!("{IMPORT}(r:read \"answer\" :count 42)\n(r:report)\n"),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "a row with no reading fails:\n{}",
        text(&out)
    );
    let t = text(&out);
    assert!(
        t.contains("missing") && t.contains("never read"),
        "the failure names the row nobody read:\n{t}"
    );
}

#[test]
fn the_instrument_proves_a_gauge_live_before_its_first_reading() {
    let b = Bench::new(
        "proven",
        "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]\n\
         [\"dropped\" :objects 0]",
        &format!(
            "{IMPORT}(r:delta \"dropped\" (fn [] {{:x 1}}) :on [r:objects] :n 50)\n(r:report)\n"
        ),
    );
    let out = b.run();
    assert!(
        out.status.success(),
        "a dropped struct reads 0 on a live gauge:\n{}",
        text(&out)
    );
    let printed = lines(&out);
    assert_eq!(
        printed.len(),
        2,
        "the discriminator and the reading:\n{}",
        text(&out)
    );
    assert!(
        printed[0].contains("\"subject\":\"objects gauge (live-growth)\"")
            && printed[0].contains("\"kind\":\"floor\"")
            && printed[0].contains("\"verdict\":\"ok\""),
        "the gauge is proven live first, against its floor:\n{}",
        printed[0]
    );
    assert!(
        printed[1].contains("\"subject\":\"dropped\"") && printed[1].contains("\"verdict\":\"ok\""),
        "then the producer's own reading:\n{}",
        printed[1]
    );
}

#[test]
fn a_dead_gauge_voids_every_reading_on_its_axis() {
    // The trap: a gauge that reads flat and a shape that reclaims both read 0,
    // so a dead gauge paints every control green. The growth row is what
    // tells them apart, and a reading of 0 on the dead axis must not pass.
    let b = Bench::new(
        "flat",
        "[\"widgets gauge (live-growth)\" :widgets :floor 0.5 :class :growth]\n\
         [\"thing\" :widgets 0]",
        &format!(
            "{IMPORT}(def g (r:gauge :widgets \"widgets/op\" (fn [] 0) :disc (fn [j] j) :floor 0.5))\n\
             (r:delta \"thing\" (fn [] 1) :on [g] :n 10)\n(r:report)\n"
        ),
    );
    let out = b.run();
    assert!(
        !out.status.success(),
        "a dead gauge fails the run:\n{}",
        text(&out)
    );
    let printed = lines(&out);
    assert_eq!(
        printed.len(),
        2,
        "the discriminator and the reading:\n{}",
        text(&out)
    );
    assert!(
        printed[0].contains("\"verdict\":\"void\""),
        "the flat discriminator is void:\n{}",
        printed[0]
    );
    assert!(
        printed[1].contains("\"subject\":\"thing\"") && printed[1].contains("\"verdict\":\"void\""),
        "and so is the reading of 0 that followed it on that axis:\n{}",
        printed[1]
    );
}

#[test]
fn a_program_with_no_path_prints_readings_and_judges_nothing() {
    // A form the runner runs in a worker thread was started with no path, so
    // it has no producer and no ledger. It prints, the runner judges.
    let out = eval("(def r ((import \"std/ratchet\"))) (r:read \"answer\" :count 42) (r:report)");
    assert!(
        out.status.success(),
        "nothing to judge, nothing to fail:\n{}",
        text(&out)
    );
    let printed = lines(&out);
    assert_eq!(
        printed.len(),
        1,
        "the reading is still printed:\n{}",
        text(&out)
    );
    assert!(
        printed[0].contains("\"subject\":\"answer\"") && !printed[0].contains("\"verdict\""),
        "without a bound or a verdict:\n{}",
        printed[0]
    );
}
