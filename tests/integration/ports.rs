use crate::common::eval_source;

#[test]
fn test_port_open_file_display() {
    let result =
        eval_source("(string (port/open \"/tmp/elle-test-port-display-474\" :write))").unwrap();
    result
        .with_string(|s| {
            assert!(s.starts_with("#<port:file"), "got: {}", s);
            assert!(s.contains(":write"), "got: {}", s);
            assert!(s.contains(":text"), "got: {}", s);
        })
        .unwrap();
}

#[test]
fn test_port_open_bytes_display() {
    let result =
        eval_source("(string (port/open-bytes \"/tmp/elle-test-port-bytes-474\" :write))").unwrap();
    result
        .with_string(|s| {
            assert!(s.starts_with("#<port:file"), "got: {}", s);
            assert!(s.contains(":binary"), "got: {}", s);
        })
        .unwrap();
}

// --- type-of ---

#[test]
fn test_port_type_of() {
    // ExternalObject type_name is "port", so type-of returns :port
    let result = eval_source("(type (port/stdin))").unwrap();
    assert_eq!(result.as_keyword_name(), Some("port"));
}
