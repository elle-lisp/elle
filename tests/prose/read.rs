// audited: 2026-10-20
// The readers the prose checks share: which files claim the policy, and the
// links, spans, call-outs and header paths a check reads out of one.
// docs/impl/audit.md

use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file in the repository whose stamp claims the policy, with its text.
///
/// `--others --exclude-standard` is why a file written a minute ago is swept
/// before it is committed. Tracked-only would exempt every new file from its
/// own policy until after it landed, which is the one moment the checks are
/// there to cover — and the sweep would report success having read nothing.
pub fn stamped() -> Vec<(String, String)> {
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
        if claims(&text) {
            found.push((rel.to_string(), text));
        }
    }
    assert!(
        !found.is_empty(),
        "no stamped files found; the sweep would pass vacuously"
    );
    found
}

/// Whether `text` carries a stamp that claims the checkable rules: a plain
/// stamp in its first eight lines. A deviation stamp, `audited: <date> (#N)`,
/// names the issue behind a rule the file breaks, so it claims none of them.
pub fn claims(text: &str) -> bool {
    text.lines()
        .take(8)
        .any(|l| match l.split_once("audited: ") {
            Some((_, rest)) => !rest
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '-')
                .starts_with(" (#"),
            None => false,
        })
}

/// Document paths named in one header comment line.
pub fn header_doc_paths(line: &str) -> Vec<String> {
    let t = line.trim_start();
    if !t.starts_with("//") && !t.starts_with('#') {
        return Vec::new();
    }
    let mut out = Vec::new();
    for word in t.split_whitespace() {
        // Brackets are trimmed with the rest of the punctuation: a header cites a
        // document mid-sentence as often as on a line of its own, and
        // `(docs/impl/x.md § …)` names the same file `docs/impl/x.md` does.
        let w = word.trim_matches(|c: char| {
            !c.is_ascii_graphic() || matches!(c, '`' | ',' | '(' | ')' | '[' | ']')
        });
        if w.ends_with(".md") && w.contains('/') && !w.starts_with('.') {
            out.push(w.to_string());
        }
    }
    out
}

/// The first prose sentence in a source file's header block, skipping the
/// stamp and any bare document path.
pub fn source_callout(text: &str) -> Option<String> {
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
pub fn links_in(text: &str) -> Vec<String> {
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
pub fn code_spans(line: &str) -> Vec<String> {
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
pub fn callout(text: &str) -> Option<String> {
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

#[test]
fn a_deviation_stamp_claims_none_of_the_checkable_rules() {
    // The counter-factual: read every stamp as a claim and a file that names
    // the issue behind its deviation fails the very check that issue tracks,
    // so the only stamp it could carry is none.
    assert!(claims("// audited: 2026-10-19\n// A call-out.\n"));
    assert!(claims("# T\n\n<!-- audited: 2026-10-19 -->\n"));
    assert!(!claims("// audited: 2026-10-19 (#1400)\n// A call-out.\n"));
    assert!(!claims("# T\n\n<!-- audited: 2026-10-19 (#1400) -->\n"));
    assert!(!claims("// no stamp at all\n"));
}
