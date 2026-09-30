// audited: 2026-09-30
// A producer judges every reading against the ledger, and a direct run is the
// whole gate for that producer.
//
// docs/ratchet.md
//
// The counter-factual: a pin is a literal in the producer and "shrink-only" is
// a comment beside it, so nothing fails when the pin is raised, a fix that
// reclaims more leaves a loose pin behind, and a subject nobody measures any
// more is a silence nobody can see. These drive a scratch producer against a
// scratch ledger and read the verdicts off its exit status and its lines.

use std::path::{Path, PathBuf};
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

const IMPORT: &str = "(def r ((import \"std/ratchet\")))\n";

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
fn a_reading_past_its_pin_the_better_way_is_stale() {
    // The trap this pins: a pin left loose is a ratchet that slipped. A later
    // regression back to 43 would pass a pin of 43, so the loose pin fails
    // until it is moved.
    let b = Bench::new(
        "better",
        "[\"answer\" :count 43]",
        &format!("{IMPORT}(r:read \"answer\" :count 42)\n(r:report)\n"),
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
    let out = Command::new(elle_binary())
        .args([
            "-e",
            "(def r ((import \"std/ratchet\"))) (r:read \"answer\" :count 42) (r:report)",
        ])
        .current_dir(repo_root())
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e");
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

#[test]
fn the_root_is_the_tree_the_binary_was_built_in() {
    let out = Command::new(elle_binary())
        .args(["-e", "(println (elle/root))"])
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e");
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let got = Path::new(&printed).canonicalize().expect("the root exists");
    let want = repo_root().canonicalize().expect("the manifest dir exists");
    assert_eq!(got, want, "elle/root names the repository root");
}
