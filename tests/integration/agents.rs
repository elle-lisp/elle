// audited: 2026-09-05
// The generated index, specified in docs/impl/agents.md.
//
// scripts/agents builds each directory's AGENTS.md from the call-out sentence
// of every document beneath it. These tests pin the extraction rules against
// fixture trees, because the failure mode they guard is silent: a generator
// that drops a document produces a shorter index that still looks correct, and
// the reader who needed that document goes to a search instead.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn script() -> PathBuf {
    repo_root().join("scripts/agents")
}

/// A fixture tree under the platform temp root, removed when the test ends.
/// Uniquely named: fixed scratch names collide across concurrent runs.
struct Tree(PathBuf);

impl Tree {
    fn new(tag: &str) -> Tree {
        let dir = std::env::temp_dir().join(format!(
            "elle-agents-{}-{}-{:?}",
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

    /// The index this tree's contents produce, as `scripts/agents --print`
    /// writes it. `--root` is what lets the generator run against a fixture
    /// instead of the repository it lives in.
    fn index(&self, dir: &str) -> String {
        let out = Command::new(script())
            .args(["--root", self.0.to_str().expect("utf-8 path"), "--print", dir])
            .output()
            .expect("run scripts/agents");
        assert!(
            out.status.success(),
            "scripts/agents failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("utf-8 output")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn index_lists_every_document_in_its_directory() {
    let t = Tree::new("lists-all");
    t.write("docs/one.md", "# One\n\nThe first subject.\n")
        .write("docs/two.md", "# Two\n\nThe second subject.\n")
        .write("docs/three.md", "# Three\n\nThe third subject.\n");

    let index = t.index("docs");
    for name in ["one.md", "two.md", "three.md"] {
        assert!(index.contains(name), "index omits {name}:\n{index}");
    }
}

#[test]
fn callout_is_the_first_sentence_under_the_title() {
    let t = Tree::new("first-sentence");
    // The docs hard-wrap, so the call-out spans source lines and has to be
    // joined before it is cut. Reading one line would truncate at the wrap.
    t.write(
        "docs/wrapped.md",
        "# Wrapped\n\nThe sentence continues\nacross a line break. A second sentence follows.\n",
    );

    let index = t.index("docs");
    assert!(
        index.contains("The sentence continues across a line break."),
        "call-out was not joined across the wrap:\n{index}"
    );
    assert!(
        !index.contains("A second sentence"),
        "call-out took more than the first sentence:\n{index}"
    );
}

#[test]
fn callout_over_budget_is_listed_as_more() {
    let t = Tree::new("over-budget");
    let long = "x".repeat(200);
    t.write("docs/long.md", &format!("# Long\n\n{long}.\n"));

    let index = t.index("docs");
    assert!(
        index.contains("(more...)"),
        "a call-out over 140 characters must degrade to (more...):\n{index}"
    );
    assert!(
        index.contains("long.md"),
        "an over-budget document stays listed; silence sends the reader to a search:\n{index}"
    );
    assert!(
        !index.contains(&long),
        "the over-budget sentence must not be printed:\n{index}"
    );
}

#[test]
fn document_with_no_callout_is_listed_as_more() {
    let t = Tree::new("no-callout");
    t.write("docs/bare.md", "# Bare\n\n## Straight to a heading\n\nBody.\n");

    let index = t.index("docs");
    assert!(
        index.contains("bare.md") && index.contains("(more...)"),
        "a document with no call-out is listed as uncalled, which is the work queue:\n{index}"
    );
}

#[test]
fn subdirectory_is_summarised_by_its_overview_callout() {
    let t = Tree::new("overview");
    t.write("docs/impl/overview.md", "# Implementation\n\nHow the compiler is built.\n")
        .write("docs/impl/lir.md", "# LIR\n\nSSA form and virtual registers.\n")
        .write("docs/top.md", "# Top\n\nA document at the parent level.\n");

    let index = t.index("docs");
    assert!(
        index.contains("How the compiler is built."),
        "a child directory is summarised by its overview.md call-out:\n{index}"
    );
}

#[test]
fn subdirectory_without_overview_lists_child_titles() {
    let t = Tree::new("no-overview");
    t.write("docs/impl/lir.md", "# LIR\n\nSSA form.\n")
        .write("docs/impl/vm.md", "# VM\n\nThe dispatch loop.\n");

    let index = t.index("docs");
    // Worse than a summary, better than a bare directory name, and it needs no
    // judgment from the generator. A bare name would be the wrong answer: it
    // costs the reader a descent to learn nothing.
    assert!(
        index.contains("LIR") && index.contains("VM"),
        "without an overview.md the parent lists the child titles:\n{index}"
    );
}

#[test]
fn index_links_to_its_parent() {
    let t = Tree::new("parent-link");
    t.write("docs/impl/lir.md", "# LIR\n\nSSA form.\n");

    let index = t.index("docs/impl");
    assert!(
        index.contains(".."),
        "every index but the root links upward, or the tree is only walkable downward:\n{index}"
    );
}

#[test]
fn references_are_links_not_bare_names() {
    let t = Tree::new("links");
    t.write("docs/one.md", "# One\n\nThe first subject.\n");

    let index = t.index("docs");
    // DOCUMENTATION.md requires markdown links for every file reference. An
    // index of bare code-span names is the exact shape that rotted in
    // docs/AGENTS.md, where no tool could see the references at all.
    assert!(
        index.contains("](") && index.contains("one.md"),
        "index entries are markdown links:\n{index}"
    );
}

#[test]
fn the_generator_refuses_to_overwrite_a_handwritten_index() {
    // The trap this pins: the obvious generator walks the tree and writes
    // every AGENTS.md it finds a directory for. Run once against this
    // repository, that destroys thousands of lines of hand-written module
    // knowledge, and the loss looks like a successful build.
    let t = Tree::new("no-clobber");
    let handwritten = "# io\n\nHand-written module knowledge that predates the generator.\n";
    t.write("docs/one.md", "# One\n\nThe first subject.\n")
        .write("docs/AGENTS.md", handwritten);

    let out = Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path")])
        .output()
        .expect("run scripts/agents");

    let after = fs::read_to_string(t.0.join("docs/AGENTS.md")).expect("index still readable");
    assert_eq!(
        after, handwritten,
        "a hand-written index must survive a generator run"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("docs")
            || String::from_utf8_lossy(&out.stderr).contains("docs"),
        "the generator names the directory it declined to take"
    );
}

#[test]
fn the_generator_writes_where_it_already_owns_the_index() {
    let t = Tree::new("owned");
    t.write("docs/one.md", "# One\n\nThe first subject.\n");

    // No AGENTS.md at all: the generator takes the directory.
    Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path")])
        .output()
        .expect("run scripts/agents");
    let first = fs::read_to_string(t.0.join("docs/AGENTS.md")).expect("index written");
    assert!(first.contains("one.md"), "first run writes the index:\n{first}");

    // Its own marker is what lets the second run rewrite it.
    t.write("docs/two.md", "# Two\n\nThe second subject.\n");
    Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path")])
        .output()
        .expect("run scripts/agents");
    let second = fs::read_to_string(t.0.join("docs/AGENTS.md")).expect("index rewritten");
    assert!(
        second.contains("two.md"),
        "a generated index is rewritten in place:\n{second}"
    );
}

#[test]
fn check_ignores_a_directory_the_generator_does_not_own() {
    // A gate that fails on every unconverted directory is red from the day it
    // is turned on, and it names a fix the generator refuses to perform.
    let t = Tree::new("check-handwritten");
    t.write("docs/one.md", "# One\n\nThe first subject.\n")
        .write("docs/AGENTS.md", "# docs\n\nHand-written knowledge.\n");

    let out = Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path"), "--check"])
        .output()
        .expect("run scripts/agents");
    assert!(
        out.status.success(),
        "--check passes over an index the generator does not own: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn check_fails_when_the_committed_index_is_stale() {
    let t = Tree::new("check");
    // Carries the marker, so the generator owns it, and its content is wrong.
    // Without the marker the ownership rule would skip it and --check would
    // pass, which is the opposite of what this test claims.
    t.write("docs/one.md", "# One\n\nThe first subject.\n").write(
        "docs/AGENTS.md",
        "# docs\n\nGenerated by `scripts/agents`. Edit a document, never this index.\n\nstale body\n",
    );

    let out = Command::new(script())
        .args(["--root", t.0.to_str().expect("utf-8 path"), "--check", "docs"])
        .output()
        .expect("run scripts/agents");
    assert!(
        !out.status.success(),
        "--check must exit non-zero on a stale index, or CI cannot gate on it"
    );
}

#[test]
fn every_document_in_this_repository_has_a_callout_within_budget() {
    // The standing check over the real tree. It reports rather than fails
    // while the migration in docs/impl/agents.md is in progress; the count is
    // the work queue, and it only goes down.
    let root = repo_root();
    let mut over = Vec::new();
    let out = Command::new("git")
        .args(["ls-files", "*.md"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    for rel in String::from_utf8_lossy(&out.stdout).lines() {
        if rel.ends_with("AGENTS.md") || rel.ends_with("CLAUDE.md") {
            continue;
        }
        let text = match fs::read_to_string(root.join(rel)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Some(callout) = first_sentence_under_title(&text) {
            if callout.chars().count() > 140 {
                over.push(rel.to_string());
            }
        } else {
            over.push(rel.to_string());
        }
    }
    eprintln!("documents without a call-out within budget: {}", over.len());
}

/// The first sentence of the first paragraph under `# Title`, joined across
/// hard wraps. Returns None when the document has no such paragraph.
fn first_sentence_under_title(text: &str) -> Option<String> {
    let mut lines = text.lines().skip_while(|l| !l.starts_with("# "));
    lines.next()?;
    let mut para = String::new();
    for line in lines {
        let t = line.trim();
        if t.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with('|') || t.starts_with('-') || t.starts_with('>') {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(t);
    }
    if para.is_empty() {
        return None;
    }
    match para.find(". ") {
        Some(i) => Some(para[..=i].to_string()),
        None => Some(para),
    }
}
