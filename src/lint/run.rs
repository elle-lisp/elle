// audited: 2026-10-26
//! CLI entry point for `elle lint`: the options, the paths a directory holds, and the exit status.
//!
//! src/lint/README.md

use crate::lint::cli::{LintConfig, Linter, OutputFormat};
use crate::lint::diagnostics::Severity;

/// Run the linter with the given arguments (everything after `lint`).
/// Returns an exit code: 0 = clean, 1 = errors, 2 = warnings only.
pub fn run(args: &[String]) -> i32 {
    let mut config = LintConfig::default();
    let mut paths = Vec::new();

    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--format" => {
                let Some(value) = args.next() else {
                    eprintln!("Error: --format expects a value");
                    return 1;
                };
                config.format = match value.as_str() {
                    "json" => OutputFormat::Json,
                    "text" | "human" => OutputFormat::Human,
                    other => {
                        eprintln!("Unknown format: {}", other);
                        return 1;
                    }
                };
            }
            "--level" => {
                let Some(value) = args.next() else {
                    eprintln!("Error: --level expects a value");
                    return 1;
                };
                config.min_severity = match value.as_str() {
                    "error" => Severity::Error,
                    "warning" => Severity::Warning,
                    "info" => Severity::Info,
                    other => {
                        eprintln!("Unknown severity: {}", other);
                        return 1;
                    }
                };
            }
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            other if other.starts_with('-') => {
                eprintln!("Unknown option: {}", other);
                return 1;
            }
            other => paths.push(other.to_string()),
        }
    }

    if paths.is_empty() {
        eprintln!("Error: no files specified");
        return 1;
    }

    let mut linter = Linter::new(config);
    let mut had_errors = false;

    for path in paths {
        let files = if crate::path::is_file(&path) {
            Ok(vec![path])
        } else if crate::path::is_dir(&path) {
            lisp_files(&path)
        } else {
            Err(format!("File not found: {}", path))
        };
        match files {
            Ok(files) => {
                for file in files {
                    if let Err(e) = linter.lint_file(&file) {
                        eprintln!("Error linting {}: {}", file, e);
                        had_errors = true;
                    }
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                had_errors = true;
            }
        }
    }

    println!("{}", linter.format_output());

    if had_errors {
        1
    } else {
        linter.exit_code()
    }
}

/// Every `.lisp` file below `dir`, sorted by path. A subdirectory whose name
/// starts with `.` is skipped, and a directory that cannot be listed is an error.
fn lisp_files(dir: &str) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    collect_lisp_files(dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn collect_lisp_files(dir: &str, found: &mut Vec<String>) -> Result<(), String> {
    let listing_error = |e: std::io::Error| format!("Error listing {}: {}", dir, e);
    for entry in std::fs::read_dir(dir).map_err(listing_error)? {
        let entry = entry.map_err(listing_error)?;
        let Some(path) = entry.path().to_str().map(|s| s.to_string()) else {
            continue; // skip non-UTF-8 paths
        };
        if crate::path::is_file(&path)
            && crate::path::extension(&path).is_some_and(|ext| ext == "lisp")
        {
            found.push(path);
        } else if crate::path::is_dir(&path)
            && !crate::path::filename(&path).is_some_and(|n| n.starts_with('.'))
        {
            collect_lisp_files(&path, found)?;
        }
    }
    Ok(())
}

fn print_help() {
    let default = LintConfig::default().min_severity;
    let levels: Vec<String> = [Severity::Error, Severity::Warning, Severity::Info]
        .iter()
        .map(|level| {
            if *level == default {
                format!("{} (default)", level)
            } else {
                level.to_string()
            }
        })
        .collect();

    println!("elle lint - Opinionated linter for Elle Lisp");
    println!();
    println!("Usage: elle lint [OPTIONS] <file|dir>...");
    println!();
    println!("Options:");
    println!("  --format <format>     Output format: text (default), json");
    println!(
        "  --level <level>       Minimum severity: {}",
        levels.join(", ")
    );
    println!("  --help, -h            Show this help message");
    println!();
    println!("Examples:");
    println!("  elle lint script.lisp");
    println!("  elle lint src/ --format json");
    println!("  elle lint script.lisp --level error");
}

#[cfg(test)]
mod tests;
