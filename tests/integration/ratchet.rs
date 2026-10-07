// audited: 2026-10-05
// The instrument measures and prints: one line per reading, carrying the
// reading alone, whatever ledger sits beside the program.
//
// docs/ratchet.md
//
// The counter-factual: the instrument found a ledger through the binary's
// build tree or ELLE_LEDGER and judged each reading as it landed, so a direct
// run printed a verdict the runner then judged again, from a second code path.
// These run a scratch producer directly with a ledger present both ways, and
// read its lines.

use std::process::{Command, Output};

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A scratch working directory holding a producer and, under
/// `tests/ledger`, a ledger that names it — the layout the runner reads.
struct Bench {
    dir: crate::common::ScratchDir,
}

impl Bench {
    /// Write `source` as `producer.lisp` and `rows` as its ledger.
    fn new(tag: &str, rows: &str, source: &str) -> Bench {
        let dir = crate::common::ScratchDir::new(&format!("ratchet-{tag}"));
        std::fs::write(dir.join("producer.lisp"), source).expect("write the producer");
        let ledger = dir.join("tests/ledger");
        std::fs::create_dir_all(&ledger).expect("create the ledger dir");
        std::fs::write(
            ledger.join("producer.lisp"),
            format!("(elle/epoch 13)\n(producer \"producer.lisp\")\n{rows}\n"),
        )
        .expect("write the ledger");
        Bench { dir }
    }

    /// Run the producer directly, as `elle producer.lisp`, from the scratch
    /// directory. `ELLE_LEDGER` names the same ledger, so neither way of
    /// finding one is left untried.
    fn run(&self) -> Output {
        Command::new(elle_binary())
            .arg("producer.lisp")
            .current_dir(self.dir.path())
            .env("ELLE_LEDGER", self.dir.join("tests/ledger"))
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run the producer")
    }
}

/// Both streams, so a failure names whatever the run printed.
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

/// The fields the runner owns, which no line the instrument prints carries.
const JUDGED: [&str; 3] = ["\"bound\"", "\"kind\"", "\"verdict\""];

fn assert_unjudged(line: &str) {
    for field in JUDGED {
        assert!(
            !line.contains(field),
            "the instrument judges nothing, so the line carries no {field}:\n{line}"
        );
    }
}

#[test]
fn the_line_carries_the_reading_alone() {
    // The row would judge the reading `regression`. A producer that judged
    // itself would fail here and print the verdict.
    let b = Bench::new(
        "line",
        "[\"answer\" :count 41]",
        &format!("{IMPORT}(r:read \"answer\" :count 42)\n"),
    );
    let out = b.run();
    assert!(
        out.status.success(),
        "a producer fails on nothing it measured:\n{}",
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
    ] {
        assert!(
            printed[0].contains(want),
            "the line carries {want}:\n{}",
            printed[0]
        );
    }
    assert_unjudged(&printed[0]);
}

#[test]
fn the_live_growth_reading_is_printed_first_with_its_class_and_floor() {
    let b = Bench::new(
        "proven",
        "[\"objects gauge (live-growth)\" :objects :floor 0.5 :class :growth]\n\
         [\"dropped\" :objects 0]",
        &format!("{IMPORT}(r:delta \"dropped\" (fn [] {{:x 1}}) :on [r:objects] :n 50)\n"),
    );
    let out = b.run();
    assert!(out.status.success(), "the producer runs:\n{}", text(&out));
    let printed = lines(&out);
    assert_eq!(
        printed.len(),
        2,
        "the discriminator and the reading:\n{}",
        text(&out)
    );
    assert!(
        printed[0].contains("\"subject\":\"objects gauge (live-growth)\"")
            && printed[0].contains("\"class\":\"growth\"")
            && printed[0].contains("\"floor\":0.5"),
        "the gauge is proven live first, naming its class and floor:\n{}",
        printed[0]
    );
    assert!(
        printed[1].contains("\"subject\":\"dropped\""),
        "then the producer's own reading:\n{}",
        printed[1]
    );
    for line in &printed {
        assert_unjudged(line);
    }
}

/// The drive section: a rate over a run-block of the caller's own. These read
/// the heap, which only an implementation test may assert.
///
/// The counter-factual: a tail recursion performs its b ops in one call, a
/// fiber is drained after b yields, and a strand needs its op run as a
/// discarded statement. No per-op thunk expresses those, so each dashboard
/// kept a private estimator for the shape.
const DRIVES: &str = "(def r ((import \"std/ratchet\")))\n\
(def @kept @[])\n\
(defn keep-n [b]\n\
  (when (%not (%int? b)) (error :b))\n\
  (def @i 0)\n\
  (while (%lt i b)\n\
    (push kept {:x i})\n\
    (assign i (%add i 1))))\n\
(defn drop-n [b]\n\
  (when (%not (%int? b)) (error :b))\n\
  (def @i 0)\n\
  (while (%lt i b)\n\
    {:x i}\n\
    (assign i (%add i 1))))\n\
(def driven (get (r:drive \"kept loop\" keep-n :block 50 :min 4 :max 10) 0))\n\
(assert (= driven:subject \"kept loop\") \"a drive's reading names its subject\")\n\
(assert (= driven:axis :objects) \"on the default gauge\")\n\
(assert (< 0.5 driven:value) \"and reads the growth the run-block makes, per op\")\n\
(def dropped (get (r:drive \"dropped loop\" drop-n :block 50 :min 4 :max 10) 0))\n\
(assert (= dropped:value 0.0) \"a loop that drops its structs reads 0\")\n\
(def two (r:drive \"two gauges\" drop-n :on [r:objects r:regions] :block 50 :min 4 :max 10))\n\
(assert (= (length two) 2) \"one reading per gauge in :on\")\n\
(assert (= (get (get two 1) :axis) :regions) \"in the order given\")\n\
(def stmt (get (r:drive \"dropped statement\" (r:stmt-run (fn [] {:y 1})) :block 50 :min 4 :max 10) 0))\n\
(assert (= stmt:subject \"dropped statement\") \"stmt-run makes a run-block of a thunk\")\n\
(assert (= stmt:value 0.0) \"and a struct dropped as a statement costs nothing\")\n\
(println \"drives: ok\")\n";

#[test]
fn a_drive_reads_the_rate_of_a_run_block_of_the_callers_own() {
    let b = Bench::new("drives", "", DRIVES);
    let out = b.run();
    assert!(
        out.status.success() && text(&out).contains("drives: ok"),
        "every drive reads what its run-block does:\n{}",
        text(&out)
    );
    for line in lines(&out) {
        assert_unjudged(&line);
    }
}

/// The value a `measure` line carries for `subject`.
fn value_of(out: &Output, subject: &str) -> String {
    let key = format!("\"subject\":\"{subject}\"");
    let line = lines(out)
        .into_iter()
        .find(|l| l.contains(&key))
        .unwrap_or_else(|| panic!("no reading for {subject}:\n{}", text(out)));
    let (_, rest) = line
        .split_once("\"value\":")
        .unwrap_or_else(|| panic!("a line with no value:\n{line}"));
    rest.split([',', '}']).next().unwrap_or("").to_string()
}

#[test]
fn the_window_claims_no_page_of_its_own() {
    // The trap: the page-claim gauge counts a page whether or not it is freed,
    // so anything the instrument itself allocates between its two readings is
    // charged to the probe. A store into an array the compiler could not type
    // went through the general `put` call, and every block read one page more
    // than its ops claimed: 1/400 per op at a block of 400.
    let b = Bench::new(
        "window",
        "",
        &format!(
            "{IMPORT}(r:rate \"nothing\" (fn [j] nil) :on [r:pages] :block 400 :min 6)\n\
             (r:delta \"nothing, delta\" (fn [] nil) :on [r:pages] :n 400)\n"
        ),
    );
    let out = b.run();
    assert!(out.status.success(), "the producer runs:\n{}", text(&out));
    assert_eq!(
        value_of(&out, "nothing"),
        "0.0",
        "a rate over a probe that does nothing claims no page:\n{}",
        text(&out)
    );
    assert_eq!(
        value_of(&out, "nothing, delta"),
        "0.0",
        "and nor does a delta over a body that does nothing:\n{}",
        text(&out)
    );
}
