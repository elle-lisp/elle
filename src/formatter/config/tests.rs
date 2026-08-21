//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_default_config() {
    let config = FormatterConfig::default();
    assert_eq!(config.indent_width, 2);
    assert_eq!(config.line_length, 80);
}

#[test]
fn test_custom_config() {
    let config = FormatterConfig::new()
        .with_indent_width(4)
        .with_line_length(100);
    assert_eq!(config.indent_width, 4);
    assert_eq!(config.line_length, 100);
}
