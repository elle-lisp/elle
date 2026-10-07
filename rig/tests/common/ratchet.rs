// audited: 2026-10-05
// The scratch producer, ledger and session store the runner's build-judging tests share.
// docs/ratchet.md
//
// Each helper writes its rows for the key this rig answers, so a test holds on
// every box CI runs, and runs the rig as the runner from a scratch working
// directory that holds `tests/ledger`.

use super::{stderr, Scratch};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

pub fn rig_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle-rig")
}

/// One line an `elle-rig -e` prints, trimmed.
pub fn eval(form: &str) -> String {
    let out = Command::new(rig_binary())
        .args(["-e", &format!("(println {form})")])
        .output()
        .expect("spawn elle-rig");
    assert!(
        out.status.success(),
        "`elle-rig -e {form}` failed:\n{}",
        stderr(&out)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The key this rig answers, which every row a test writes belongs to.
pub fn key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| eval("(elle/build)"))
}

/// The build a row with no `:build` belongs to.
pub fn reference() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| eval("(get ((import \"std/ratchet/ledger\")) :reference-build)"))
}

/// A build no box is.
pub const FOREIGN: &str = "interp-pool-plan9-mips";

/// `ROW` closed as a row of this rig's build: `["answer" :count 42` becomes
/// `["answer" :count 42 :build "KEY"]`.
pub fn own(row: &str) -> String {
    format!("{row} :build \"{}\"]", key())
}

/// `ROW` closed as a row of a build no box is.
pub fn foreign(row: &str) -> String {
    format!("{row} :build \"{FOREIGN}\"]")
}

/// `ROW` closed as `--repin` adopts it on this rig: with no `:build` on the
/// reference build, and naming this build anywhere else.
pub fn adopted(row: &str) -> String {
    if key() == reference() {
        format!("{row}]")
    } else {
        own(row)
    }
}

/// A producer of several forms, the whole-file shape the runner runs in a
/// worker under each JIT policy, that reads one number.
pub const PRODUCER: &str = "(def r ((import \"std/ratchet\")))\n\
                        (def answer 42)\n\
                        (r:read \"answer\" :count answer)\n";

/// A scratch working directory holding `producer.lisp`, its ledger under
/// `tests/ledger`, and a session DB of its own.
pub struct Bench {
    pub dir: Scratch,
}

impl Bench {
    /// `rows` is the producer's ledger, each row on a line of its own.
    pub fn new(tag: &str, rows: &[String], source: &str) -> Bench {
        let dir = Scratch::new(&format!("measure-{tag}"));
        dir.write("producer.lisp", source);
        std::fs::create_dir_all(dir.path().join("tests/ledger")).expect("create the ledger dir");
        dir.write(
            "tests/ledger/producer.lisp",
            &format!(
                "(elle/epoch 13)\n(producer \"producer.lisp\")\n{}\n",
                rows.join("\n")
            ),
        );
        Bench { dir }
    }

    pub fn db(&self) -> PathBuf {
        self.dir.path().join("s.db")
    }

    pub fn ledger(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("tests/ledger/producer.lisp"))
            .expect("read the ledger")
    }

    /// `elle-rig test ARGS producer.lisp` from the scratch directory.
    pub fn run(&self, args: &[&str]) -> Output {
        Command::new(rig_binary())
            .arg("test")
            .args(args)
            .args(["--timeout", "60000"])
            .arg("--db")
            .arg(self.db())
            .arg("producer.lisp")
            .current_dir(self.dir.path())
            .env_remove("RUST_MIN_STACK")
            .output()
            .expect("run elle-rig test")
    }

    /// The latest run's measurement rows.
    pub fn rows(&self) -> String {
        query(&self.db(), ROWS)
    }
}

/// `SQL` against the session DB at `db`, through the runner's `--query`.
pub fn query(db: &Path, sql: &str) -> String {
    let out = Command::new(rig_binary())
        .args(["test", "--query", sql])
        .arg("--db")
        .arg(db)
        .output()
        .expect("query the session DB");
    assert!(
        out.status.success(),
        "the query failed: {sql}\n{}",
        stderr(&out)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The latest run's measurement rows, joined to the file that printed them.
pub const ROWS: &str = "SELECT f.file AS file, r.tier AS tier, m.subject AS subject, \
                    m.axis AS axis, m.value AS value, m.half AS half, m.unit AS unit, \
                    m.bound AS bound, m.kind AS kind, m.verdict AS verdict \
                    FROM measurement m JOIN result r ON r.id = m.result_id \
                    JOIN form f ON f.hash = r.form_hash \
                    WHERE m.run_id = (SELECT max(id) FROM run) ORDER BY r.tier";
