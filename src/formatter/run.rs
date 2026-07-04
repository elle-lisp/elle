//! CLI entry point for `elle fmt`.

use crate::formatter::{format_code, FormatterConfig};
use crate::rewrite::run::rewrite_file;
use std::io::IsTerminal;

/// Options for fragment formatting.
struct FmtOpts {
    no_epoch: bool,
    preserve_margin: bool,
}

/// Top-level format dispatch: handles --no-epoch and --plm.
fn do_format(
    source: &str,
    file_path: &str,
    config: &FormatterConfig,
    opts: &FmtOpts,
) -> Result<String, String> {
    let (input, margin) = if opts.preserve_margin {
        strip_left_margin(source)
    } else {
        (source.to_string(), String::new())
    };

    let formatted = if opts.no_epoch {
        format_only(&input, config)?
    } else {
        rewrite_and_format(&input, file_path, config)?
    };

    Ok(if opts.preserve_margin {
        apply_left_margin(&formatted, &margin)
    } else {
        formatted
    })
}

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

/// Format-only (no epoch rewrite). Used by --no-epoch.
fn format_only(source: &str, config: &FormatterConfig) -> Result<String, String> {
    format_code(source, config)
}

/// Detect and strip a common left margin from every non-blank line.
/// Returns (stripped_source, margin_string).
fn strip_left_margin(source: &str) -> (String, String) {
    let min_indent = source
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);

    if min_indent == 0 {
        return (source.to_string(), String::new());
    }

    let margin: String = " ".repeat(min_indent);
    let stripped: String = source
        .lines()
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                &l[min_indent..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Preserve trailing newline
    let stripped = if source.ends_with('\n') && !stripped.ends_with('\n') {
        stripped + "\n"
    } else {
        stripped
    };

    (stripped, margin)
}

/// Re-apply a left margin to every non-blank line.
fn apply_left_margin(source: &str, margin: &str) -> String {
    if margin.is_empty() {
        return source.to_string();
    }
    source
        .lines()
        .map(|l| {
            if l.trim().is_empty() {
                l.to_string()
            } else {
                format!("{}{}", margin, l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if source.ends_with('\n') { "\n" } else { "" }
}

/// Run the formatter tool. Returns exit code.
///
/// Exit codes:
/// - 0 = success (all files formatted, or no changes needed in --check)
/// - 1 = error, or changes needed in --check mode
pub fn run(args: &[String]) -> i32 {
    let mut check = false;
    let mut no_epoch = false;
    let mut preserve_margin = false;
    let mut line_length: Option<usize> = None;
    let mut indent_width: Option<usize> = None;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--no-epoch" => no_epoch = true,
            "--plm" | "--preserve-left-margin" => preserve_margin = true,
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

    let opts = FmtOpts {
        no_epoch,
        preserve_margin,
    };

    // No files given: if stdin is piped, format it; otherwise show help.
    if files.is_empty() {
        if std::io::stdin().is_terminal() {
            eprintln!("Error: no files specified");
            print_help();
            return 1;
        }
        return run_stdin(check, &config, &opts);
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

        match do_format(&source, file_path, &config, &opts) {
            Ok(formatted) => {
                if check {
                    if let Some(exit) = check_columns(file_path, &formatted, config.line_length) {
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
fn run_stdin(check: bool, config: &FormatterConfig, opts: &FmtOpts) -> i32 {
    let mut source = String::new();
    if let Err(e) = std::io::Read::read_to_string(&mut std::io::stdin(), &mut source) {
        eprintln!("Error reading stdin: {}", e);
        return 1;
    }

    let formatted = match do_format(&source, "<stdin>", config, opts) {
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
fn check_columns(file_path: &str, formatted: &str, line_length: usize) -> Option<i32> {
    let mut has_warning = false;
    let mut has_error = false;
    let openers = ['(', '[', '{'];
    let warn_col = line_length * 3 / 4; // 75% of line length (60 for default 80)

    for (line_num, line) in formatted.lines().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            if openers.contains(&ch) {
                if col >= line_length {
                    eprintln!(
                        "error: {}:{}:{}: opening delimiter past column {}",
                        file_path,
                        line_num + 1,
                        col + 1,
                        line_length,
                    );
                    has_error = true;
                } else if col >= warn_col {
                    eprintln!(
                        "warning: {}:{}:{}: opening delimiter past column {}, consider refactoring",
                        file_path,
                        line_num + 1,
                        col + 1,
                        warn_col,
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
    println!("  --no-epoch                Skip epoch tag injection/upgrade (for fragments)");
    println!("  --plm                     Preserve left margin (auto-detect + re-apply indent)");
    println!("  --line-length=N           Target line length (default: 80)");
    println!("  --indent-width=N          Spaces per indent level (default: 2)");
    println!("  --help                    Show this help message");
    println!();
    println!("Examples:");
    println!("  elle fmt src/*.lisp");
    println!("  elle fmt --check lib/*.lisp");
    println!("  elle fmt --line-length=120 src/*.lisp");
    println!("  elle fmt --no-epoch --plm fragment.lisp");
    println!("  cat file.lisp | elle fmt");
}

#[cfg(test)]
mod tests;
