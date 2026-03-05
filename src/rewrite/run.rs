//! CLI entry point for `elle rewrite`.

use super::engine::rewrite_source;
use super::rule::RewriteRule;

/// Run the rewrite tool. Returns exit code.
/// Exit codes: 0 = success (or no changes in --check mode), 1 = changes needed (--check) or error.
pub fn run(args: &[String]) -> i32 {
    let mut check = false;
    let mut dry_run = false;
    let mut list_rules = false;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--dry-run" => dry_run = true,
            "--list-rules" => list_rules = true,
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            other => {
                if !other.starts_with('-') {
                    files.push(other.to_string());
                } else {
                    eprintln!("Unknown option: {}", other);
                    return 1;
                }
            }
        }
        i += 1;
    }

    if list_rules {
        let rules = build_rules();
        if rules.is_empty() {
            println!("No rewrite rules configured.");
        } else {
            for rule in &rules {
                println!("  {}", rule.name());
            }
        }
        return 0;
    }

    if files.is_empty() {
        eprintln!("Error: no files specified");
        print_help();
        return 1;
    }

    let rules = build_rules();
    let rule_refs: Vec<&dyn RewriteRule> = rules.iter().map(|r| r.as_ref()).collect();
    let mut any_changes = false;
    let mut had_errors = false;

    for file_path in &files {
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", file_path, e);
                had_errors = true;
                continue;
            }
        };

        let (result, edits) = match rewrite_source(&source, &rule_refs) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Error rewriting {}: {}", file_path, e);
                had_errors = true;
                continue;
            }
        };

        if edits.is_empty() {
            continue;
        }

        any_changes = true;

        if check {
            println!("{}: {} edit(s) needed", file_path, edits.len());
        } else if dry_run {
            println!("{}: {} edit(s) would be applied", file_path, edits.len());
            for edit in &edits {
                println!(
                    "  offset {}: {:?} -> {:?}",
                    edit.byte_offset,
                    &source[edit.byte_offset..edit.byte_offset + edit.byte_len],
                    edit.replacement,
                );
            }
        } else {
            // Write back
            if let Err(e) = std::fs::write(file_path, &result) {
                eprintln!("Error writing {}: {}", file_path, e);
                had_errors = true;
                continue;
            }
            println!("{}: {} edit(s) applied", file_path, edits.len());
        }
    }

    if had_errors {
        return 1;
    }

    if check && any_changes {
        return 1;
    }

    0
}

/// Build the set of rewrite rules. Currently empty — rules will be populated
/// by issue #471 (primitive `/` → `-` renames).
fn build_rules() -> Vec<Box<dyn RewriteRule>> {
    Vec::new()
}

fn print_help() {
    println!("elle rewrite - Source-to-source rewriting tool");
    println!();
    println!("Usage: elle rewrite [OPTIONS] <file...>");
    println!();
    println!("Options:");
    println!("  --check        Check if changes are needed (exit 1 if yes)");
    println!("  --dry-run      Show what would change without modifying files");
    println!("  --list-rules   List available rewrite rules");
    println!("  --help, -h     Show this help message");
    println!();
    println!("Examples:");
    println!("  elle rewrite script.lisp");
    println!("  elle rewrite --check src/*.lisp");
    println!("  elle rewrite --dry-run examples/");
}
