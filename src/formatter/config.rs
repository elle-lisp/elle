//! Formatter configuration
//!
//! Defines formatting style parameters.

/// Formatting configuration for Elle code
#[derive(Debug, Clone)]
pub struct FormatterConfig {
    /// Number of spaces per indentation level (default: 2)
    pub indent_width: usize,
    /// Target line length (default: 80)
    pub line_length: usize,
}

impl FormatterConfig {
    /// Create a new formatter configuration with default settings
    pub fn new() -> Self {
        Self {
            indent_width: 2,
            line_length: 80,
        }
    }

    /// Set the indentation width
    pub fn with_indent_width(mut self, width: usize) -> Self {
        self.indent_width = width;
        self
    }

    /// Set the line length
    pub fn with_line_length(mut self, length: usize) -> Self {
        self.line_length = length;
        self
    }
}

impl Default for FormatterConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
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
}
