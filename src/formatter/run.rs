//! CLI entry point for `elle fmt`.

use crate::formatter::{format_code, FormatterConfig};
use crate::rewrite::run::rewrite_file;
use std::io::IsTerminal;

/// Run epoch rewrite on source, then format.
/// Returns the formatted string with epoch migrations applied.
fn rewrite_and_format(
    source: &str,
    file_path: &str,
    config: &FormatterConfig,
) -> Result<String, String> {
    let rewritten = match rewrite_file(source, file_path)? {
        Some((new_source, _)) => new_source,
        None => source.to_string(),
    };
    format_code(&rewritten, config)
}

/// Run the formatter tool. Returns exit code.
///
/// Exit codes:
/// - 0 = success (all files formatted, or no changes needed in --check)
/// - 1 = error, or changes needed in --check mode
pub fn run(args: &[String]) -> i32 {
    let mut check = false;
    let mut line_length: Option<usize> = None;
    let mut indent_width: Option<usize> = None;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            opt if opt.starts_with("--line-length=") => {
                if let Some(val) = opt.strip_prefix("--line-length=") {
                    match val.parse::<usize>() {
                        Ok(n) => line_length = Some(n),
                        Err(_) => {
                            eprintln!("Error: --line-length expects a number, got {:?}", val);
                            return 1;
                        }
                    }
                }
            }
            opt if opt.starts_with("--indent-width=") => {
                if let Some(val) = opt.strip_prefix("--indent-width=") {
                    match val.parse::<usize>() {
                        Ok(n) => indent_width = Some(n),
                        Err(_) => {
                            eprintln!("Error: --indent-width expects a number, got {:?}", val);
                            return 1;
                        }
                    }
                }
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

    let config = FormatterConfig::new()
        .maybe_with_line_length(line_length)
        .maybe_with_indent_width(indent_width);

    // No files given: if stdin is piped, format it; otherwise show help.
    if files.is_empty() {
        if std::io::stdin().is_terminal() {
            eprintln!("Error: no files specified");
            print_help();
            return 1;
        }
        return run_stdin(check, &config);
    }

    let mut any_changed = false;
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

        match rewrite_and_format(&source, file_path, &config) {
            Ok(formatted) => {
                if check {
                    if let Some(exit) = check_columns(file_path, &formatted) {
                        had_errors = exit != 0;
                    }
                }
                if formatted == source {
                    // No changes needed
                } else if check {
                    println!("{}", file_path);
                    any_changed = true;
                } else if let Err(e) = std::fs::write(file_path, &formatted) {
                    eprintln!("Error writing {}: {}", file_path, e);
                    had_errors = true;
                }
            }
            Err(e) => {
                eprintln!("Error formatting {}: {}", file_path, e);
                had_errors = true;
            }
        }
    }

    if had_errors {
        return 1;
    }

    if check && any_changed {
        return 1;
    }

    0
}

/// Format stdin, write to stdout.
fn run_stdin(check: bool, config: &FormatterConfig) -> i32 {
    let mut source = String::new();
    if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut source) {
        eprintln!("Error reading stdin: {}", e);
        return 1;
    }

    let formatted = match rewrite_and_format(&source, "<stdin>", config) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error formatting stdin: {}", e);
            return 1;
        }
    };

    if check {
        if formatted != source {
            return 1;
        }
        return 0;
    }

    print!("{}", formatted);
    0
}

/// Check for lines with opening delimiters past column thresholds.
/// Returns Some(exit_code) if violations found, None if clean.
fn check_columns(file_path: &str, formatted: &str) -> Option<i32> {
    let mut has_warning = false;
    let mut has_error = false;
    let openers = ['(', '[', '{'];

    for (line_num, line) in formatted.lines().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if openers.contains(&ch) {
                if col >= 80 {
                    eprintln!(
                        "error: {}:{}:{}: opening delimiter past column 80",
                        file_path,
                        line_num + 1,
                        col + 1
                    );
                    has_error = true;
                } else if col >= 60 {
                    eprintln!(
                        "warning: {}:{}:{}: opening delimiter past column 60, consider refactoring",
                        file_path,
                        line_num + 1,
                        col + 1
                    );
                    has_warning = true;
                }
            }
        }
    }

    if has_error {
        Some(1)
    } else if has_warning {
        Some(0)
    } else {
        None
    }
}

fn print_help() {
    println!("elle fmt - Opinionated code formatter");
    println!();
    println!("Formats Elle source files in place. One canonical style.");
    println!("No configuration beyond line width and indent width.");
    println!();
    println!("Usage: elle fmt [OPTIONS] <file...>");
    println!("       elle fmt < file.lisp");
    println!();
    println!("Options:");
    println!("  --check                   Check if files need formatting (exit 1 if yes)");
    println!("  --line-length=N           Target line length (default: 80)");
    println!("  --indent-width=N          Spaces per indent level (default: 2)");
    println!("  --help                    Show this help message");
    println!();
    println!("Examples:");
    println!("  elle fmt src/*.lisp");
    println!("  elle fmt --check lib/*.lisp");
    println!("  elle fmt --line-length=120 src/*.lisp");
    println!("  cat file.lisp | elle fmt");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::epoch::CURRENT_EPOCH;

    #[test]
    fn test_fmt_upgrades_epoch() {
        let config = FormatterConfig::default();
        let input = "(elle/epoch 0)\n(assert-true x \"test\")\n";
        let result = rewrite_and_format(input, "<test>", &config).unwrap();
        assert!(
            result.contains(&format!("(elle/epoch {})", CURRENT_EPOCH)),
            "should upgrade epoch tag, got: {:?}",
            result
        );
        assert!(
            !result.contains("assert-true"),
            "old symbol should be rewritten, got: {:?}",
            result
        );
    }

    #[test]
    fn test_fmt_current_epoch_unchanged() {
        let config = FormatterConfig::default();
        let input = format!("(elle/epoch {})\n(println \"hello\")\n", CURRENT_EPOCH);
        let result = rewrite_and_format(&input, "<test>", &config).unwrap();
        assert_eq!(result, input, "current-epoch file should be unchanged");
    }

    #[test]
    fn test_fmt_no_epoch_gets_one() {
        let config = FormatterConfig::default();
        let input = "(println \"hello\")\n";
        let result = rewrite_and_format(input, "<test>", &config).unwrap();
        assert!(
            result.contains(&format!("(elle/epoch {})", CURRENT_EPOCH)),
            "should inject epoch tag, got: {:?}",
            result
        );
    }

    #[test]
    fn test_fmt_epoch_upgrade_idempotent() {
        let config = FormatterConfig::default();
        let input = "(elle/epoch 0)\n(assert-true x \"test\")\n";
        let first = rewrite_and_format(input, "<test>", &config).unwrap();
        let second = rewrite_and_format(&first, "<test>", &config).unwrap();
        assert_eq!(first, second, "rewrite+format must be idempotent");
    }
}
