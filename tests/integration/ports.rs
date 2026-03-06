use crate::common::eval_source;

// --- Display ---

#[test]
fn test_port_stdin_display() {
    let result = eval_source("(string (port/stdin))").unwrap();
    result
        .with_string(|s| {
            assert_eq!(s, "#<port:stdin>");
        })
        .unwrap();
}

#[test]
fn test_port_stdout_display() {
    let result = eval_source("(string (port/stdout))").unwrap();
    result
        .with_string(|s| {
            assert_eq!(s, "#<port:stdout>");
        })
        .unwrap();
}

#[test]
fn test_port_stderr_display() {
    let result = eval_source("(string (port/stderr))").unwrap();
    result
        .with_string(|s| {
            assert_eq!(s, "#<port:stderr>");
        })
        .unwrap();
}
