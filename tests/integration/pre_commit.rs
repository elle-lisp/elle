// audited: 2026-09-30
// The pre-commit hook formats what a commit stages, and never leaves the real index behind HEAD.
//
// CONTRIBUTING.md
//
// Each test builds a scratch repository that runs the real .githooks/pre-commit
// against the built `elle`. The audit gate and the index generator are stubbed:
// their own tests live in audit.rs and agents.rs.
//
// The trap every test here guards: `git commit -- <paths>` builds a temporary
// index and runs the hook against it. A `git add` in the hook reaches only that
// index, so the commit takes the formatted file while the real index keeps the
// unformatted one. `git status` then shows `MM`, and the next commit taken from
// the index silently reverts the formatting.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn elle() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// A formatted Lisp file at the current epoch.
fn formatted(source: &str) -> String {
    let mut child = Command::new(elle())
        .arg("fmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn elle fmt");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(source.as_bytes())
        .expect("write to elle fmt");
    let out = child.wait_with_output().expect("wait for elle fmt");
    assert!(out.status.success(), "elle fmt failed on {source:?}");
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// A formatted file, whose body the hook leaves alone.
const TIDY: &str = "(defn f [x]\n  (+ x 1))\n";

/// The same file, which the hook reformats.
const UNTIDY: &str = "(defn f [x]     (+ x 1))\n";

/// A second formatted body, for a change the hook leaves alone.
const OTHER: &str = "(defn g [y]\n  (* y 2))\n";

/// The audit gate and the index generator, stubbed. `scripts/agents` copies
/// `AGENTS.next` over `AGENTS.md` when a test has written one, which stands in
/// for an index the generator rewrote.
const AUDIT_STUB: &str = "#!/bin/sh\nexit 0\n";
const AGENTS_STUB: &str = "#!/bin/sh\n[ -f AGENTS.next ] && cp AGENTS.next AGENTS.md\nexit 0\n";

/// A scratch repository under the platform temp root, removed when the test
/// ends. Uniquely named: fixed scratch names collide across concurrent runs.
struct Repo(PathBuf);

impl Repo {
    fn new(tag: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!(
            "elle-pre-commit-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create the scratch repository");
        let repo = Repo(dir);
        repo.must(&["init", "-q", "-b", "main"]);
        repo.must(&["config", "user.name", "Test"]);
        repo.must(&["config", "user.email", "test@example.invalid"]);
        repo.must(&["config", "commit.gpgsign", "false"]);
        repo.must(&["config", "core.hooksPath", ".githooks"]);
        let hook =
            fs::read_to_string(repo_root().join(".githooks/pre-commit")).expect("read the hook");
        repo.script(".githooks/pre-commit", &hook);
        repo.script("scripts/audit", AUDIT_STUB);
        repo.script("scripts/agents", AGENTS_STUB);
        repo.write(".gitignore", "target/\nAGENTS.next\n");
        repo.must(&["add", "."]);
        repo.must(&["commit", "-q", "--no-verify", "-m", "scaffold"]);
        repo
    }

    fn write(&self, rel: &str, body: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().expect("has a parent")).expect("create a directory");
        fs::write(&path, body).expect("write a file");
    }

    /// Commit `body` as `rel` without the hook. A partial commit names only
    /// tracked paths, so each test seeds the file it later changes.
    fn seed(&self, rel: &str, body: &str) {
        self.write(rel, body);
        self.must(&["add", rel]);
        self.must(&["commit", "-q", "--no-verify", "-m", "seed"]);
    }

    fn script(&self, rel: &str, body: &str) {
        self.write(rel, body);
        let path = self.0.join(rel);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod +x");
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(rel)).expect("read a file")
    }

    /// Git in the scratch repository, isolated from the caller's configuration
    /// and from any repository the test itself runs inside.
    fn git(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .env("ELLE", elle())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("CARGO_TARGET_DIR", self.0.join("target"))
            .env_remove("GIT_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_WORK_TREE")
            .output()
            .expect("run git")
    }

    fn must(&self, args: &[&str]) -> String {
        let out = self.git(args);
        assert!(
            out.status.success(),
            "git {args:?} failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8 output")
    }

    /// A file as HEAD holds it.
    fn head(&self, rel: &str) -> String {
        self.must(&["show", &format!("HEAD:{rel}")])
    }

    /// Commit with the hook enabled.
    fn commit(&self, extra: &[&str]) -> Output {
        let mut args = vec!["commit", "-q", "-m", "change"];
        args.extend_from_slice(extra);
        self.git(&args)
    }

    /// The invariant under test: nothing staged differs from HEAD. A real
    /// index behind HEAD is exactly a staged difference.
    fn assert_index_at_head(&self) {
        let staged = self.must(&["diff", "--cached", "--name-only", "HEAD"]);
        assert!(
            staged.is_empty(),
            "the real index differs from HEAD in:\n{staged}\nstatus:\n{}",
            self.must(&["status", "--short"])
        );
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What a command wrote, stdout then stderr. `git commit` reports "nothing to
/// commit" on stdout, and the hook reports a refusal on stderr.
fn transcript(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_plain_commit_stages_the_reformatted_file() {
    let r = Repo::new("plain");
    r.write("f.lisp", UNTIDY);
    r.must(&["add", "f.lisp"]);

    let out = r.commit(&[]);
    assert!(
        out.status.success(),
        "the commit failed: {}",
        transcript(&out)
    );
    assert_eq!(r.head("f.lisp"), formatted(UNTIDY));
    assert_ne!(r.head("f.lisp"), UNTIDY, "the fixture must need formatting");
    assert_eq!(r.read("f.lisp"), r.head("f.lisp"));
    r.assert_index_at_head();
}

#[test]
fn commit_all_stages_the_reformatted_file() {
    // `git commit -a` also builds its own index, but that index becomes the
    // real one, so the hook may stage into it and the commit goes through.
    let r = Repo::new("all");
    r.seed("f.lisp", &formatted(TIDY));
    r.write("f.lisp", UNTIDY);

    let out = r.commit(&["-a"]);
    assert!(
        out.status.success(),
        "the commit failed: {}",
        transcript(&out)
    );
    assert_eq!(r.head("f.lisp"), formatted(UNTIDY));
    r.assert_index_at_head();
}

#[test]
fn a_plain_commit_migrates_an_older_epoch() {
    // Epoch 10 renamed `cons` to `pair`. The hook runs `elle fmt` without
    // `--no-epoch`, so the commit lands the migrated file.
    let old = "(elle/epoch 9)\n(def p (cons 1 2))\n";
    let r = Repo::new("migrate");
    r.write("f.lisp", old);
    r.must(&["add", "f.lisp"]);

    let out = r.commit(&[]);
    assert!(
        out.status.success(),
        "the commit failed: {}",
        transcript(&out)
    );
    let head = r.head("f.lisp");
    assert!(
        head.contains("(pair 1 2)") && !head.contains("elle/epoch 9"),
        "the hook migrates the file: {head:?}"
    );
    assert_eq!(head, formatted(old));
    r.assert_index_at_head();
}

#[test]
fn a_partial_commit_of_a_formatted_file_goes_through() {
    // The counter-factual: a hook that refuses every partial commit passes the
    // tests below and makes `git commit -- <paths>` unusable.
    let r = Repo::new("partial-tidy");
    r.seed("f.lisp", &formatted(TIDY));
    let tidy = formatted(OTHER);
    r.write("f.lisp", &tidy);

    let out = r.commit(&["--", "f.lisp"]);
    assert!(
        out.status.success(),
        "the commit failed: {}",
        transcript(&out)
    );
    assert_eq!(r.head("f.lisp"), tidy);
    r.assert_index_at_head();
}

#[test]
fn a_partial_commit_the_hook_reformats_is_refused_and_formatted_on_disk() {
    let r = Repo::new("partial-refused");
    r.seed("f.lisp", &formatted(TIDY));
    let before = r.must(&["rev-parse", "HEAD"]);
    r.write("f.lisp", UNTIDY);

    let out = r.commit(&["--", "f.lisp"]);
    assert!(
        !out.status.success(),
        "the hook must refuse a partial commit it reformatted"
    );
    assert!(
        transcript(&out).contains("f.lisp"),
        "the refusal names the file: {}",
        transcript(&out)
    );
    assert_eq!(r.must(&["rev-parse", "HEAD"]), before, "nothing committed");
    assert_eq!(
        r.read("f.lisp"),
        formatted(UNTIDY),
        "the hook formats the file on disk"
    );
    r.assert_index_at_head();
}

#[test]
fn a_partial_commit_never_leaves_the_real_index_behind_head() {
    // Running the same commit again takes the file the hook formatted, and
    // the real index agrees with what landed.
    let r = Repo::new("partial-again");
    // The seed differs from the formatted change, or the second attempt has
    // nothing to commit.
    r.seed("f.lisp", &formatted(OTHER));
    r.write("f.lisp", UNTIDY);

    let first = r.commit(&["--", "f.lisp"]);
    if first.status.success() {
        r.assert_index_at_head();
    }
    let second = r.commit(&["--", "f.lisp"]);
    assert!(
        second.status.success(),
        "the second attempt commits the formatted file: {}",
        transcript(&second)
    );
    assert_eq!(r.head("f.lisp"), formatted(UNTIDY));
    assert_eq!(r.read("f.lisp"), r.head("f.lisp"));
    r.assert_index_at_head();
}

#[test]
fn a_partial_commit_the_hook_migrates_is_refused() {
    let old = "(elle/epoch 9)\n(def p (cons 1 2))\n";
    let r = Repo::new("partial-migrate");
    r.seed("f.lisp", &formatted(TIDY));
    r.write("f.lisp", old);

    let out = r.commit(&["--", "f.lisp"]);
    assert!(
        !out.status.success(),
        "the hook must refuse a partial commit it migrated"
    );
    assert_eq!(r.read("f.lisp"), formatted(old));
    r.assert_index_at_head();
}

#[test]
fn a_partial_commit_refuses_an_index_the_generator_rewrote() {
    // The generator rewrites an AGENTS.md the commit does not name. Staging it
    // into the temporary index would commit it past the real index, so the
    // hook refuses until the author names it too.
    let r = Repo::new("partial-agents");
    r.seed("AGENTS.md", "old\n");
    r.seed("f.lisp", &formatted(TIDY));
    r.write("AGENTS.next", "new\n");
    r.write("f.lisp", &formatted(OTHER));

    let out = r.commit(&["--", "f.lisp"]);
    assert!(
        !out.status.success(),
        "the hook must refuse a partial commit that leaves the regenerated index out"
    );
    assert!(
        transcript(&out).contains("AGENTS.md"),
        "the refusal names the index: {}",
        transcript(&out)
    );
    r.assert_index_at_head();

    let out = r.commit(&["--", "f.lisp", "AGENTS.md"]);
    assert!(
        out.status.success(),
        "naming the index commits it: {}",
        transcript(&out)
    );
    assert_eq!(r.head("AGENTS.md"), "new\n");
    r.assert_index_at_head();
}

#[test]
fn a_partial_commit_of_an_unformatted_rust_file_is_refused() {
    // The Rust path runs `cargo fmt` over the workspace, so the scratch
    // repository has to be a crate. Its first commit carries a formatted
    // crate so that clippy and rustdoc have nothing to object to.
    let r = Repo::new("partial-rust");
    r.seed(
        "Cargo.toml",
        "[package]\nname = \"scratch\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    r.seed(
        "src/lib.rs",
        "//! Scratch.\n\n/// One.\npub fn one() -> u32 {\n    1\n}\n",
    );
    let untidy = "//! Scratch.\n\n/// Two.\npub fn two()->u32{ 2 }\n";
    r.write("src/lib.rs", untidy);

    let out = r.commit(&["--", "src/lib.rs"]);
    assert!(
        !out.status.success(),
        "the hook must refuse a partial commit it reformatted"
    );
    assert_ne!(r.read("src/lib.rs"), untidy, "cargo fmt ran on disk");
    r.assert_index_at_head();

    let out = r.commit(&["--", "src/lib.rs"]);
    assert!(
        out.status.success(),
        "the second attempt commits the formatted file: {}",
        transcript(&out)
    );
    assert_eq!(r.read("src/lib.rs"), r.head("src/lib.rs"));
    r.assert_index_at_head();
}
