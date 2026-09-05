// audited: 2026-09-05
// The audit queue and its commit gate, specified in docs/impl/audit.md.
//
// scripts/audit answers two questions: did the files this commit stages carry
// today's stamp, and what should be read next. The second is the one with a
// wrong obvious answer — sorting by stamp age puts recently-read files at the
// back and never-audited-but-harmless leaves at the front, which is close to
// backwards. These tests pin the cost ordering so that cannot regress to age.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script() -> PathBuf {
    repo_root().join("scripts/audit")
}

/// A fixture tree under the platform temp root, removed when the test ends.
/// Uniquely named: fixed scratch names collide across concurrent runs.
struct Tree(PathBuf);

impl Tree {
    fn new(tag: &str) -> Tree {
        let dir = std::env::temp_dir().join(format!(
            "elle-audit-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create fixture root");
        Tree(dir)
    }

    fn write(&self, rel: &str, body: &str) -> &Tree {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().expect("has a parent")).expect("create fixture dir");
        fs::write(&path, body).expect("write fixture file");
        self
    }

    /// The queue, one path per line, most costly first.
    fn queue(&self) -> Vec<String> {
        let out = Command::new(script())
            .args(["--root", self.0.to_str().expect("utf-8 path"), "--all"])
            .output()
            .expect("run scripts/audit");
        assert!(
            out.status.success(),
            "scripts/audit failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout)
            .expect("utf-8 output")
            .lines()
            .filter_map(|l| l.split_whitespace().last().map(str::to_string))
            .collect()
    }

    fn rank_of(&self, needle: &str) -> usize {
        let q = self.queue();
        q.iter()
            .position(|p| p.ends_with(needle))
            .unwrap_or_else(|| panic!("{needle} is absent from the queue: {q:?}"))
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A document of roughly `bytes` length, carrying `stamp` if one is given.
fn doc(stamp: Option<&str>, bytes: usize) -> String {
    let head = match stamp {
        Some(d) => format!("# Title\n\n<!-- audited: {d} -->\n\n"),
        None => "# Title\n\n".to_string(),
    };
    let body = "filler text on a line.\n".repeat(bytes / 23 + 1);
    head + &body
}

#[test]
fn an_unstamped_file_outranks_a_stamped_one_of_equal_cost() {
    let t = Tree::new("unstamped-first");
    t.write("a.md", &doc(None, 400))
        .write("b.md", &doc(Some("2026-09-04"), 400));

    assert!(
        t.rank_of("a.md") < t.rank_of("b.md"),
        "a file that has never been audited comes first at equal cost"
    );
}

#[test]
fn the_queue_ranks_by_cost_not_by_stamp_age() {
    // The counter-factual this test exists for: order by stamp age alone and
    // the deep tiny file wins, because "never audited" sorts before any date.
    // It is also the file whose staleness costs nobody anything. The shallow
    // document is read by every session that opens the tree.
    let t = Tree::new("cost-over-age");
    t.write("shallow.md", &doc(Some("2026-01-01"), 20_000))
        .write("a/b/c/d/deep.md", &doc(None, 200));

    assert!(
        t.rank_of("shallow.md") < t.rank_of("deep.md"),
        "a large shallow document outranks a small deep one, whatever their stamps say"
    );
}

#[test]
fn a_shallow_file_outranks_a_deep_file_of_the_same_size() {
    let t = Tree::new("depth");
    t.write("top.md", &doc(None, 2_000))
        .write("a/b/c/buried.md", &doc(None, 2_000));

    assert!(
        t.rank_of("top.md") < t.rank_of("buried.md"),
        "read frequency falls with depth, so depth discounts the cost of staleness"
    );
}

#[test]
fn a_large_file_outranks_a_small_file_at_the_same_depth() {
    let t = Tree::new("size");
    t.write("big.md", &doc(None, 20_000))
        .write("small.md", &doc(None, 100));

    assert!(
        t.rank_of("big.md") < t.rank_of("small.md"),
        "more stale prose in one file costs the reader more"
    );
}

#[test]
fn a_stamp_naming_an_issue_counts_as_stamped() {
    let t = Tree::new("deviation");
    t.write("tracked.md", &doc(Some("2026-09-04 (#123)"), 400))
        .write("bare.md", &doc(None, 400));

    // `audited: <date> (#N)` reads as audited with a known deviation, tracked
    // where the work is scheduled. Treating it as unstamped would push every
    // deliberately-deferred file to the front of the queue forever.
    assert!(
        t.rank_of("bare.md") < t.rank_of("tracked.md"),
        "a deviation with an issue behind it is not the same as never audited"
    );
}

#[test]
fn a_generated_index_carries_no_stamp_and_is_not_queued() {
    let t = Tree::new("generated");
    t.write(
        "AGENTS.md",
        "# root\n\nGenerated by `scripts/agents`. Edit a document, never this index.\n",
    )
    .write("real.md", &doc(None, 100));

    let q = t.queue();
    assert!(
        !q.iter().any(|p| p.ends_with("AGENTS.md")),
        "a generated index is exempt; its generator is what gets audited. Queue: {q:?}"
    );
}

#[test]
fn a_handwritten_index_is_queued_like_any_other_document() {
    // The counter-factual: exempt the name AGENTS.md and every hand-written
    // index leaves the queue, the root one included. That file is read every
    // session, so it is the costliest place in the tree for a stale claim.
    let t = Tree::new("handwritten-index");
    t.write("AGENTS.md", &doc(None, 8_000))
        .write("small.md", &doc(None, 100));

    let q = t.queue();
    assert!(
        q.iter().any(|p| p.ends_with("AGENTS.md")),
        "a hand-written index carries knowledge somebody wrote, so it is audited: {q:?}"
    );
}

#[test]
fn the_staged_gate_fails_a_file_with_no_stamp_for_today() {
    let t = Tree::new("gate-fail");
    t.write("changed.md", &doc(Some("2020-01-01"), 400));

    let out = Command::new(script())
        .args([
            "--root",
            t.0.to_str().expect("utf-8 path"),
            "--staged",
            "changed.md",
        ])
        .output()
        .expect("run scripts/audit");
    assert!(
        !out.status.success(),
        "a file changed today with a stale stamp must fail the gate"
    );
}

#[test]
fn the_staged_gate_passes_a_file_stamped_today() {
    let t = Tree::new("gate-pass");
    let today = String::from_utf8(
        Command::new("date")
            .arg("+%F")
            .output()
            .expect("date")
            .stdout,
    )
    .expect("utf-8")
    .trim()
    .to_string();
    t.write("changed.md", &doc(Some(&today), 400));

    let out = Command::new(script())
        .args([
            "--root",
            t.0.to_str().expect("utf-8 path"),
            "--staged",
            "changed.md",
        ])
        .output()
        .expect("run scripts/audit");
    assert!(
        out.status.success(),
        "stamped today must pass: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn the_gate_and_the_queue_agree_on_what_is_exempt() {
    // The trap: two copies of the eligibility test drift, and the pair that
    // results is silent. A file the gate exempts but the queue counts is
    // reported as unaudited forever and no commit can ever clear it.
    let t = Tree::new("agree");
    t.write(
        "AGENTS.md",
        "# root\n\nGenerated by `scripts/agents`. Edit a document, never this index.\n",
    );

    let out = Command::new(script())
        .args([
            "--root",
            t.0.to_str().expect("utf-8 path"),
            "--staged",
            "AGENTS.md",
        ])
        .output()
        .expect("run scripts/audit");
    let queued = t.queue().iter().any(|p| p.ends_with("AGENTS.md"));
    assert!(
        out.status.success() && !queued,
        "AGENTS.md must be exempt from the gate and absent from the queue, or neither"
    );
}

#[test]
fn a_directory_carrying_its_own_licence_is_vendored_and_not_queued() {
    let t = Tree::new("vendored");
    t.write("vendor/tables.rs", &doc(None, 8_000))
        .write("vendor/LICENSE-MIT", "MIT\n")
        .write("ours.rs", &doc(None, 200));

    let q = t.queue();
    assert!(
        !q.iter().any(|p| p.contains("vendor/")),
        "a vendored tree is not ours to audit: {q:?}"
    );
    assert!(
        q.iter().any(|p| p.ends_with("ours.rs")),
        "our own files stay queued: {q:?}"
    );
}

#[test]
fn a_licence_at_the_root_exempts_nothing() {
    // The counter-factual: walk every ancestor including the root and the
    // repository's own LICENSE empties the queue. That failure is silent —
    // the queue reports zero files and reads as a tree fully in policy.
    let t = Tree::new("root-licence");
    t.write("LICENSE", "MIT\n").write("ours.md", &doc(None, 400));

    assert!(
        t.queue().iter().any(|p| p.ends_with("ours.md")),
        "the root licence covers the repository; it does not exempt it"
    );
}

#[test]
fn files_stamped_before_the_policy_are_reported_as_out_of_policy() {
    // The stamp records which version of the policy a file met, not how
    // recently somebody read it. A file read yesterday against last year's
    // rules has still never seen the current ones, and only the policy's own
    // stamp can tell the two apart.
    let t = Tree::new("policy-floor");
    t.write(
        "DOCUMENTATION.md",
        "# Documentation policy\n\n<!-- audited: 2026-06-01 -->\n\nThe rules.\n",
    )
    .write("stale.md", &doc(Some("2026-05-31"), 400))
    .write("current.md", &doc(Some("2026-06-02"), 400));

    let out = Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path"), "--policy"])
        .output()
        .expect("run scripts/audit");
    let listed = String::from_utf8(out.stdout).expect("utf-8 output");

    assert!(
        listed.contains("stale.md"),
        "a file stamped one day before the policy has not met it:\n{listed}"
    );
    assert!(
        !listed.contains("current.md"),
        "a file stamped after the policy has met it:\n{listed}"
    );
}

#[test]
fn the_policy_floor_is_not_just_recency() {
    // The counter-factual: rank by date alone and these two sort the same way
    // whatever the policy says, so the test would pass against an
    // implementation that never reads DOCUMENTATION.md at all.
    let t = Tree::new("policy-not-recency");
    t.write(
        "DOCUMENTATION.md",
        "# Documentation policy\n\n<!-- audited: 2020-01-01 -->\n\nThe rules.\n",
    )
    .write("old-but-current.md", &doc(Some("2020-06-01"), 400));

    let out = Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path"), "--policy"])
        .output()
        .expect("run scripts/audit");
    let listed = String::from_utf8(out.stdout).expect("utf-8 output");

    assert!(
        !listed.contains("old-but-current.md"),
        "an old stamp that still postdates the policy is in policy:\n{listed}"
    );
}

#[test]
fn the_scripts_use_no_construct_that_fails_on_macos() {
    // CI runs macos-latest (.github/workflows/pr.yml). The trap is that every
    // one of these works here and fails there, so a Linux-only run is green
    // and the macOS job is the first thing to see it.
    //
    //   date -d      GNU only; BSD date spells it `-j -f`
    //   mapfile      bash 4+; macOS ships bash 3.2 as /bin/bash
    //   grep -P      GNU only
    //   sed -i       BSD sed requires an argument to -i
    //   readlink -f  GNU only
    const GNUISMS: &[&str] = &["date -d", "mapfile", "grep -P", "sed -i ", "readlink -f"];
    // GNU extensions to basic regular expressions. BSD sed reads each as the
    // literal character, so the pattern quietly stops matching rather than
    // erroring — `s|^$root/\?||` leaves every path absolute on macOS and the
    // caller builds nonsense from it.
    const BRE: &[&str] = &["\\?", "\\+", "\\|"];
    let mut found = Vec::new();
    for name in ["scripts/audit", "scripts/agents"] {
        let text = fs::read_to_string(repo_root().join(name)).expect("read the script");
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with('#') {
                continue;
            }
            for g in GNUISMS {
                if line.contains(g) {
                    found.push(format!("{name}:{}: {g}", n + 1));
                }
            }
            if line.contains("sed ") {
                for b in BRE {
                    if line.contains(b) {
                        found.push(format!("{name}:{}: GNU BRE `{b}` in sed", n + 1));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "GNU-only constructs: {found:#?}");
}

#[test]
fn the_queue_dates_files_without_calling_date_on_a_stamp() {
    // Day arithmetic on an ISO stamp is done in awk so it needs no `date -d`.
    // This pins the behavior the portable implementation has to keep: an older
    // stamp ranks above a newer one at equal cost.
    let t = Tree::new("date-math");
    t.write("older.md", &doc(Some("2001-02-03"), 400))
        .write("newer.md", &doc(Some("2025-12-31"), 400));

    assert!(
        t.rank_of("older.md") < t.rank_of("newer.md"),
        "a stamp from 2001 is staler than one from 2025"
    );
}

#[test]
fn documentation_policy_carries_its_own_stamp() {
    // The policy that requires a stamp is the first file that has to have one.
    let text = fs::read_to_string(repo_root().join("DOCUMENTATION.md")).expect("read the policy");
    assert!(
        text.lines().take(8).any(|l| l.contains("audited: ")),
        "DOCUMENTATION.md carries an audit stamp under its title"
    );
}
