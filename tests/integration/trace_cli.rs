// audited: 2026-09-28
// The CLI rejects unknown trace keywords and lists the accepted names.
//
// docs/config.md

use std::process::Command;

#[test]
fn an_unknown_trace_keyword_fails_before_running_the_program() {
    let output = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(["--trace=call,nonsense", "-e", "(println \"ran\")"])
        .output()
        .expect("spawn elle");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let valid = elle::config::TRACE_KEYWORDS.join(", ");

    assert!(
        !output.status.success(),
        "unknown trace keyword was accepted"
    );
    assert!(
        output.stdout.is_empty(),
        "program ran before CLI validation"
    );
    assert!(
        stderr.contains("--trace: unknown keyword 'nonsense'")
            && stderr.contains(&format!("Valid: {valid}")),
        "expected a useful error listing trace keywords, got:\n{stderr}"
    );
}
