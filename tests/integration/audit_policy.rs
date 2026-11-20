// audited: 2026-10-22
// The policy date, which the audit queue reads off DOCUMENTATION.md as the floor of the second count.
//
// docs/impl/audit.md
//
// A stamp records which version of the policy a file met. The policy is
// audited like any other file, so its own stamp moves with every edit to it.
// Only a rule change may move the floor: a floor read off the stamp returns to
// the queue every file stamped since the last rule change, on an edit that
// fixed a link.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::{write_fixture, ScratchDir};

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/audit")
}

/// A policy stamped on `audited` whose rules last changed on `date`.
fn policy(audited: &str, date: &str) -> String {
    format!("# Policy\n\n<!-- audited: {audited} -->\n<!-- policy: {date} -->\n\nThe rules.\n")
}

/// A document stamped on `date`.
fn doc(date: &str) -> String {
    format!("# Title\n\n<!-- audited: {date} -->\n\nbody\n")
}

/// `scripts/audit --root ROOT ARGS…`, however it exits.
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(script())
        .args(["--root", root.to_str().expect("utf-8 path")])
        .args(args)
        .output()
        .expect("run scripts/audit")
}

/// The stdout of `scripts/audit --root ROOT ARGS…`, which must succeed.
fn audit(root: &Path, args: &[&str]) -> String {
    let out = run(root, args);
    assert!(
        out.status.success(),
        "scripts/audit {args:?} failed with {:?}:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

#[test]
fn files_stamped_before_the_policy_date_are_reported_as_out_of_policy() {
    // A file read yesterday against last year's rules has still never seen the
    // current ones, and only the policy date can tell the two apart.
    let t = ScratchDir::new("audit-policy-floor");
    write_fixture(
        t.path(),
        "DOCUMENTATION.md",
        &policy("2026-06-01", "2026-06-01"),
    );
    write_fixture(t.path(), "stale.md", &doc("2026-05-31"));
    write_fixture(t.path(), "current.md", &doc("2026-06-02"));

    let listed = audit(t.path(), &["--policy"]);
    assert!(
        listed.contains("stale.md"),
        "a file stamped one day before the policy date has not met it:\n{listed}"
    );
    assert!(
        !listed.contains("current.md"),
        "a file stamped after the policy date has met it:\n{listed}"
    );
}

#[test]
fn the_policy_floor_is_not_just_recency() {
    // The counter-factual: rank by date alone and these two sort the same way
    // whatever the policy says, so the test would pass against an
    // implementation that never reads DOCUMENTATION.md at all.
    let t = ScratchDir::new("audit-policy-recency");
    write_fixture(
        t.path(),
        "DOCUMENTATION.md",
        &policy("2020-01-01", "2020-01-01"),
    );
    write_fixture(t.path(), "old-but-current.md", &doc("2020-06-01"));

    let listed = audit(t.path(), &["--policy"]);
    assert!(
        !listed.contains("old-but-current.md"),
        "an old stamp that still postdates the policy date is in policy:\n{listed}"
    );
}

#[test]
fn the_floor_is_the_policy_date_not_the_policy_s_stamp() {
    // The counter-factual: a floor read off the stamp puts `between.md` before
    // the policy, though the edit that restamped the policy changed no rule.
    let t = ScratchDir::new("audit-policy-date");
    write_fixture(
        t.path(),
        "DOCUMENTATION.md",
        &policy("2026-06-10", "2026-06-01"),
    );
    write_fixture(t.path(), "before.md", &doc("2026-05-31"));
    write_fixture(t.path(), "between.md", &doc("2026-06-05"));

    assert_eq!(
        audit(t.path(), &["--counts"]),
        "unstamped 0\nbefore-policy 1\ndeviation 0\n",
        "one file predates the policy date; the one stamped since is in policy"
    );
    let listed = audit(t.path(), &["--policy"]);
    assert!(
        listed.contains("before.md") && !listed.contains("between.md"),
        "--policy lists the file before the policy date alone:\n{listed}"
    );
}

#[test]
fn a_policy_with_no_policy_date_is_refused() {
    // The counter-factual: a script that falls back to the stamp reads the
    // wrong floor and prints a count, so the first edit to the policy moves
    // the second count with nothing to say why.
    let t = ScratchDir::new("audit-policy-missing");
    write_fixture(
        t.path(),
        "DOCUMENTATION.md",
        "# Policy\n\n<!-- audited: 2026-06-01 -->\n\nThe rules.\n",
    );
    write_fixture(t.path(), "a.md", &doc("2026-05-31"));

    for flag in ["--counts", "--policy"] {
        let out = run(t.path(), &[flag]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && stderr.contains("policy date"),
            "{flag} refuses a policy with no policy date, and says so:\n{stderr}"
        );
    }
}

#[test]
fn documentation_policy_carries_a_policy_date_no_later_than_its_stamp() {
    // A rule change is an edit, and every edit stamps the policy, so a policy
    // date past the stamp names a rule change nobody audited.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("DOCUMENTATION.md");
    let text = std::fs::read_to_string(path).expect("read the policy");
    let date_after = |marker: &str| {
        text.lines()
            .take(8)
            .find_map(|l| l.split_once(marker).map(|(_, rest)| rest[..10].to_string()))
            .unwrap_or_else(|| panic!("DOCUMENTATION.md carries `{marker}` in its header"))
    };
    let stamp = date_after("audited: ");
    let floor = date_after("policy: ");
    assert!(
        floor <= stamp,
        "the policy date {floor} is no later than the stamp {stamp}"
    );
}
