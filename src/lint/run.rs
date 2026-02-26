//! CLI entry point for `elle --lint`.

use crate::lint::cli::{LintConfig, Linter, OutputFormat};
use crate::lint::diagnostics::Severity;
use std::path::Path;

/// Run the linter with the given arguments (everything after `--lint`).
/// Returns an exit code: 0 = clean, 1 = errors, 2 = warnings only.
pub fn run(args: &[String]) -> i32 {
    let mut format = OutputFormat::Human;
    let mut min_severity = Severity::Info;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                i += 1;
                if i < args.len() {
                    format = match args[i].as_str() {
                        "json" => OutputFormat::Json,
                        "text" | "human" => OutputFormat::Human,
                        other => {
                            eprintln!("Unknown format: {}", other);
                            return 1;
                        }
                    };
                }
            }
            "--level" => {
                i += 1;
                if i < args.len() {
                    min_severity = match args[i].as_str() {
                        "error" => Severity::Error,
                        "warning" => Severity::Warning,
                        "info" => Severity::Info,
                        other => {
                            eprintln!("Unknown severity: {}", other);
                            return 1;
                        }
                    };
                }
            }
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            other => {
                if !other.starts_with('-') {
                    files.push(other.to_string());
                }
            }
        }
        i += 1;
    }

    if files.is_empty() {
        eprintln!("Error: no files specified");
        return 1;
    }

    let config = LintConfig {
        min_severity,
        format,
    };
    let mut linter = Linter::new(config);
    let mut had_errors = false;

    for file_path in files {
        let path = Path::new(&file_path);
        if path.is_file() {
            if let Err(e) = linter.lint_file(path) {
                eprintln!("Error linting {}: {}", file_path, e);
                had_errors = true;
            }
        } else if path.is_dir() {
            lint_directory(&mut linter, path);
        } else {
            eprintln!("File not found: {}", file_path);
            had_errors = true;
        }
    }

    println!("{}", linter.format_output());

    if had_errors {
        1
    } else {
        linter.exit_code()
    }
}

fn lint_directory(linter: &mut Linter, dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "l") {
                let _ = linter.lint_file(&path);
            } else if path.is_dir()
                && !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                lint_directory(linter, &path);
            }
        }
    }
}

fn print_help() {
    println!("elle --lint - Opinionated linter for Elle Lisp");
    println!();
    println!("Usage: elle --lint [OPTIONS] <file|dir>...");
    println!();
    println!("Options:");
    println!("  --format <format>     Output format: text (default), json");
    println!("  --level <level>       Minimum severity: error, warning (default), info");
    println!("  --help, -h            Show this help message");
    println!();
    println!("Examples:");
    println!("  elle --lint script.lisp");
    println!("  elle --lint src/ --format json");
    println!("  elle --lint script.l --level error");
}
