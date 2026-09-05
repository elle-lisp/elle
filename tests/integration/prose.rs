// audited: 2026-09-05
// The parts of DOCUMENTATION.md a machine can check, run against every file
// that carries an audit stamp.
//
// The scoping is the point (docs/impl/audit.md). An unstamped file has claimed
// nothing and is exempt. A stamped file has claimed to meet the policy, so a
// violation in it is a false claim and fails the build. Coverage grows as the
// tree comes into policy, with no flag day.
//
// This exists so that the stamp cannot be discharged by counting lines. What
// a build can decide, a build decides; what is left for the reader to attest
// is whether the content is true, earns its length, and belongs here.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file in the repository carrying an audit stamp, with its text.
///
/// `--others --exclude-standard` is why a file written a minute ago is swept
/// before it is committed. Tracked-only would exempt every new file from its
/// own policy until after it landed, which is the one moment the checks are
/// there to cover — and the sweep would report success having read nothing.
fn stamped() -> Vec<(String, String)> {
    let root = repo_root();
    let out = Command::new("git")
        .args(["ls-files", "--cached", "--others", "--exclude-standard"])
        .current_dir(&root)
        .output()
        .expect("git ls-files");
    let mut found = Vec::new();
    for rel in String::from_utf8_lossy(&out.stdout).lines() {
        let text = match fs::read_to_string(root.join(rel)) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if text.lines().take(8).any(|l| l.contains("audited: ")) {
            found.push((rel.to_string(), text));
        }
    }
    assert!(
        !found.is_empty(),
        "no stamped files found; the sweep would pass vacuously"
    );
    found
}

/// Fails when a sweep's filter left it nothing to look at.
///
/// Every check here narrows `stamped()` further — to documents, to Rust, to
/// headers naming a path. A filter that matches nothing reports success, and
/// success from an empty sweep is the failure this repository is least able
/// to see.
fn examined(n: usize, what: &str) {
    assert!(n > 0, "the {what} sweep examined no files, so it proved nothing");
}

#[test]
fn every_stamped_file_is_within_the_reading_budget() {
    let mut over = Vec::new();
    let mut seen = 0;
    for (rel, text) in stamped() {
        seen += 1;
        let n = text.lines().count();
        if n > 500 {
            over.push(format!("{rel}: {n} lines"));
        }
    }
    examined(seen, "reading budget");
    assert!(
        over.is_empty(),
        "stamped files past the 500-line reading budget: {over:#?}"
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
    assert!(broken.is_empty(), "broken links in stamped documents: {broken:#?}");
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
                const GENERIC: &[&str] =
                    &["AGENTS.md", "README.md", "CLAUDE.md", "overview.md"];
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
        if rel == "DOCUMENTATION.md" || rel == "tests/integration/prose.rs" {
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
    assert!(hits.is_empty(), "banned register in stamped files: {hits:#?}");
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
            Some(c) if c.chars().count() > 140 => {
                bad.push(format!("{rel}: call-out is {} characters", c.chars().count()))
            }
            Some(_) => {}
        }
    }
    examined(seen, "document call-out");
    assert!(bad.is_empty(), "call-out problems in stamped documents: {bad:#?}");
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
            Some(c) if c.chars().count() > 140 => {
                bare.push(format!("{rel}: call-out is {} characters", c.chars().count()))
            }
            Some(_) => {}
        }
    }
    examined(seen, "source call-out");
    assert!(bare.is_empty(), "stamped source files without a call-out: {bare:#?}");
}

/// Document paths named in one header comment line.
fn header_doc_paths(line: &str) -> Vec<String> {
    let t = line.trim_start();
    if !t.starts_with("//") && !t.starts_with('#') {
        return Vec::new();
    }
    let mut out = Vec::new();
    for word in t.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_ascii_graphic() || c == '`' || c == ',');
        if w.ends_with(".md") && w.contains('/') && !w.starts_with('.') {
            out.push(w.to_string());
        }
    }
    out
}

/// The first prose sentence in a source file's header block, skipping the
/// stamp and any bare document path.
fn source_callout(text: &str) -> Option<String> {
    let mut para = String::new();
    for line in text.lines() {
        let t = line.trim_start();
        let body = if let Some(r) = t.strip_prefix("//!") {
            r
        } else if let Some(r) = t.strip_prefix("//") {
            r
        } else {
            break;
        }
        .trim();
        if body.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        // Skip the stamp, and skip a line that is only a document path. A
        // prose sentence that happens to name one is still the call-out.
        if body.starts_with("audited:") || header_doc_paths(line) == vec![body.to_string()] {
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(body);
    }
    if para.is_empty() {
        return None;
    }
    Some(match para.find(". ") {
        Some(i) => para[..=i].to_string(),
        None => para,
    })
}

/// Relative link targets in markdown, ignoring the anchor.
///
/// Fenced blocks are skipped. A fence holds a syntax example, and the paths in
/// one are illustrative — `DOCUMENTATION.md` demonstrates the link form with a
/// path that deliberately belongs to no repository. A checker that reads
/// fences reports those, and a checker that cries wolf gets switched off.
fn links_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let b = line.as_bytes();
        let mut i = 0;
        while i + 1 < b.len() {
            if b[i] == b']' && b[i + 1] == b'(' {
                if let Some(end) = line[i + 2..].find(')') {
                    let target = &line[i + 2..i + 2 + end];
                    let target = target.split('#').next().unwrap_or("");
                    if !target.is_empty() {
                        out.push(target.to_string());
                    }
                    i += 2 + end;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

/// The contents of every single-backtick span on one line.
fn code_spans(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut parts = line.split('`');
    parts.next();
    while let Some(inner) = parts.next() {
        out.push(inner.to_string());
        if parts.next().is_none() {
            break;
        }
    }
    out
}

/// The first sentence of the first paragraph under `# Title`, joined across
/// hard wraps. None when the document opens straight into structure.
fn callout(text: &str) -> Option<String> {
    let mut lines = text.lines().skip_while(|l| !l.starts_with("# "));
    lines.next()?;
    let mut para = String::new();
    for line in lines {
        let t = line.trim();
        if t.starts_with("<!--") {
            continue;
        }
        if t.is_empty() {
            if !para.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with('#') || t.starts_with('|') || t.starts_with('-') || t.starts_with('>') {
            if para.is_empty() {
                return None;
            }
            break;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(t);
    }
    if para.is_empty() {
        return None;
    }
    Some(match para.find(". ") {
        Some(i) => para[..=i].to_string(),
        None => para,
    })
}
