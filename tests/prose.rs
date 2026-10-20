// audited: 2026-10-20
// The parts of DOCUMENTATION.md a machine can check, run against every file
// whose stamp claims the policy.
//
// QA runs these checks before anything else, so they are a binary of their
// own: QA does not wait for the shared test binary to compile
// (docs/analysis/ci.md).
//
// The scoping is the point (docs/impl/audit.md). An unstamped file has claimed
// nothing and is exempt, and so is a file whose deviation stamp names the
// issue behind the rule it breaks. A plainly stamped file has claimed to meet
// the policy, so a violation in it is a false claim and fails the build.
// Coverage grows as the tree comes into policy, with no flag day.
//
// This exists so that the stamp cannot be discharged by measuring size. What
// a build can decide, a build decides; what is left for the reader to attest
// is whether the content is true, earns its length, and belongs here.

#[path = "prose/read.rs"]
mod read;
#[path = "prose/sections.rs"]
mod sections;

use read::{callout, code_spans, header_doc_paths, links_in, repo_root, source_callout, stamped};

/// Fails when a sweep's filter left it nothing to look at.
///
/// Every check here narrows `stamped()` further — to documents, to Rust, to
/// headers naming a path. A filter that matches nothing reports success, and
/// success from an empty sweep is the failure this repository is least able
/// to see.
fn examined(n: usize, what: &str) {
    assert!(
        n > 0,
        "the {what} sweep examined no files, so it proved nothing"
    );
}

#[test]
fn every_link_in_a_stamped_document_resolves() {
    let root = repo_root();
    let mut broken = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        if !rel.ends_with(".md") {
            continue;
        }
        seen += 1;
        let dir = root.join(&rel);
        let dir = dir.parent().expect("has a parent");
        for target in links_in(&text) {
            if target.starts_with("http") || target.starts_with("mailto") {
                continue;
            }
            if !dir.join(&target).exists() {
                broken.push(format!("{rel} -> {target}"));
            }
        }
    }
    examined(seen, "link");
    assert!(
        broken.is_empty(),
        "broken links in stamped documents: {broken:#?}"
    );
}

#[test]
fn no_stamped_document_names_a_file_as_a_bare_code_span() {
    // DOCUMENTATION.md requires a markdown link for every file reference. A
    // bare `name.md` costs the reader a search and no tool can resolve it,
    // which is the shape that rotted docs/AGENTS.md.
    let mut bare = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        if !rel.ends_with(".md") {
            continue;
        }
        seen += 1;
        for (n, line) in text.lines().enumerate() {
            // Skip fenced examples: a syntax demonstration is not a reference.
            if line.starts_with("```") || line.starts_with("yes:") || line.starts_with("no:") {
                continue;
            }
            for span in code_spans(line) {
                // `.md` alone is an extension. A bare index name denotes the
                // kind of file rather than one file — "read the AGENTS.md"
                // means whichever one is nearest, and has no target to link.
                const GENERIC: &[&str] = &["AGENTS.md", "README.md", "CLAUDE.md", "overview.md"];
                let is_path = span.ends_with(".md")
                    && !span.starts_with('.')
                    && !span.contains(' ')
                    // `<dir>/overview.md` is a placeholder, not a path.
                    && !span.contains('<')
                    && !GENERIC.contains(&span.as_str());
                if is_path && !line.contains(&format!("]({span})")) {
                    bare.push(format!("{rel}:{}: `{span}`", n + 1));
                }
            }
        }
    }
    examined(seen, "bare filename");
    assert!(
        bare.is_empty(),
        "file names written as code spans instead of links: {bare:#?}"
    );
}

#[test]
fn no_stamped_file_uses_the_banned_register() {
    const BANNED: &[&str] = &[
        "load-bearing",
        "smoking gun",
        "delve",
        "productive tension",
        "recalibrate",
        "put differently",
        "to be candid",
        "stepping back",
        "zooming out",
        "the key distinction is",
        "that's on me",
        "you're right to push back",
        "i should have caught that",
    ];
    let mut hits = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        seen += 1;
        // A rule's definition sites have to spell the words the rule bans:
        // the policy states the list, and this file implements it. Everywhere
        // else, naming one is using one.
        if rel == "DOCUMENTATION.md" || rel == "tests/prose.rs" {
            continue;
        }
        let lower = text.to_lowercase();
        for word in BANNED {
            if lower.contains(word) {
                hits.push(format!("{rel}: \"{word}\""));
            }
        }
    }
    examined(seen, "banned register");
    assert!(
        hits.is_empty(),
        "banned register in stamped files: {hits:#?}"
    );
}

#[test]
fn every_stamped_document_opens_with_a_callout_within_budget() {
    let mut bad = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        if !rel.ends_with(".md") || rel.ends_with("AGENTS.md") {
            continue;
        }
        seen += 1;
        match callout(&text) {
            None => bad.push(format!("{rel}: no call-out under its title")),
            Some(c) if c.chars().count() > 140 => bad.push(format!(
                "{rel}: call-out is {} characters",
                c.chars().count()
            )),
            Some(_) => {}
        }
    }
    examined(seen, "document call-out");
    assert!(
        bad.is_empty(),
        "call-out problems in stamped documents: {bad:#?}"
    );
}

#[test]
fn every_document_a_stamped_source_file_names_still_exists() {
    // No comment syntax carries a markdown link, so a source header names its
    // governing document as a plain path. That makes it the one reference in
    // the tree that only a test can check, and the shape that goes wrong in
    // bulk: one commit moves a document, and every header naming it dangles
    // at once, silently.
    let root = repo_root();
    let mut dangling = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        if rel.ends_with(".md") {
            continue;
        }
        seen += 1;
        for line in text.lines().take(12) {
            for path in header_doc_paths(line) {
                if !root.join(&path).exists() {
                    dangling.push(format!("{rel} -> {path}"));
                }
            }
        }
    }
    examined(seen, "source header");
    assert!(
        dangling.is_empty(),
        "source headers naming documents that are gone: {dangling:#?}"
    );
}

#[test]
fn every_stamped_source_file_opens_with_a_callout() {
    // A reader reaches a source file from a stack trace, a grep or a call
    // site — with less context than one who picked a document off an index,
    // and nothing to have prepared them. The header sentence is that reader's
    // only orientation.
    let mut bare = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        if rel.ends_with(".md") || !rel.ends_with(".rs") {
            continue;
        }
        seen += 1;
        match source_callout(&text) {
            None => bare.push(format!("{rel}: no call-out in its header")),
            Some(c) if c.chars().count() > 140 => bare.push(format!(
                "{rel}: call-out is {} characters",
                c.chars().count()
            )),
            Some(_) => {}
        }
    }
    examined(seen, "source call-out");
    assert!(
        bare.is_empty(),
        "stamped source files without a call-out: {bare:#?}"
    );
}
