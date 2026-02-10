//! Core formatting logic
//!
//! Implements a recursive pretty-printer for s-expressions.

use super::config::FormatterConfig;
use crate::reader::Lexer;
use crate::symbol::SymbolTable;
use crate::value::Value;
use crate::Reader;

/// Format Elle code with the given configuration
///
/// Parses the input code and returns a formatted version.
/// Returns an error if parsing fails.
pub fn format_code(source: &str, config: &FormatterConfig) -> Result<String, String> {
    // Parse the source code
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();

    loop {
        match lexer.next_token() {
            Ok(Some(token)) => {
                tokens.push(crate::reader::OwnedToken::from(token));
            }
            Ok(None) => break,
            Err(e) => return Err(format!("Lexer error: {}", e)),
        }
    }

    let mut reader = Reader::new(tokens);
    let mut symbol_table = SymbolTable::new();
    let mut values = Vec::new();

    while let Some(result) = reader.try_read(&mut symbol_table) {
        match result {
            Ok(value) => values.push(value),
            Err(e) => return Err(format!("Reader error: {}", e)),
        }
    }

    // Format each value
    let mut formatted = Vec::new();
    for value in values {
        formatted.push(format_value(&value, 0, config, &symbol_table));
    }

    Ok(formatted.join("\n"))
}

/// Format a single Value into a string
fn format_value(
    value: &Value,
    indent: usize,
    config: &FormatterConfig,
    symbol_table: &SymbolTable,
) -> String {
    match value {
        Value::Nil => "nil".to_string(),
        Value::Bool(b) => if *b { "#t" } else { "#f" }.to_string(),
        Value::Int(n) => n.to_string(),
        Value::Float(n) => n.to_string(),
        Value::String(s) => format!("\"{}\"", s.escape_default()),
        Value::Symbol(id) => symbol_table
            .name(*id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("#{}", id.0)),
        Value::Keyword(id) => symbol_table
            .name(*id)
            .map(|s| format!(":{}", s))
            .unwrap_or_else(|| format!(":{}", id.0)),
        Value::Vector(elements) => {
            if elements.is_empty() {
                "[]".to_string()
            } else {
                let items: Vec<String> = elements
                    .iter()
                    .map(|e| format_value(e, indent, config, symbol_table))
                    .collect();
                format!("[{}]", items.join(" "))
            }
        }
        Value::Table(_) => {
            // For Phase 1, just return a placeholder
            "{{...}}".to_string()
        }
        Value::Struct(_) => {
            // For Phase 1, just return a placeholder
            "{{...}}".to_string()
        }
        Value::Cons(cons_rc) => {
            format_cons(&cons_rc.first, &cons_rc.rest, indent, config, symbol_table)
        }
        Value::Closure(_) => "#<closure>".to_string(),
        Value::NativeFn(_) => "#<native-fn>".to_string(),
        Value::LibHandle(_) => "#<lib-handle>".to_string(),
        Value::CHandle(_) => "#<c-handle>".to_string(),
        Value::Exception(_) => "#<exception>".to_string(),
        Value::Condition(_) => "#<condition>".to_string(),
        Value::ThreadHandle(_) => "#<thread-handle>".to_string(),
        Value::Cell(_) => "#<cell>".to_string(),
    }
}

/// Format a cons cell (list)
fn format_cons(
    head: &Value,
    tail: &Value,
    indent: usize,
    config: &FormatterConfig,
    symbol_table: &SymbolTable,
) -> String {
    // Collect all elements in the list
    let mut elements = vec![head];
    let mut current = tail;

    loop {
        match current {
            Value::Nil => break,
            Value::Cons(cons_rc) => {
                elements.push(&cons_rc.first);
                current = &cons_rc.rest;
            }
            _ => {
                // Improper list - not common in well-formed Elle code
                elements.push(current);
                break;
            }
        }
    }

    // Try to format on one line first
    let one_line = format_list_inline(&elements, config, symbol_table);

    if one_line.len() <= config.line_length - indent {
        return one_line;
    }

    // Multi-line formatting
    format_list_multiline(&elements, indent, config, symbol_table)
}

/// Format a list on a single line
fn format_list_inline(
    elements: &[&Value],
    config: &FormatterConfig,
    symbol_table: &SymbolTable,
) -> String {
    let formatted: Vec<String> = elements
        .iter()
        .map(|e| format_value(e, 0, config, symbol_table))
        .collect();
    format!("({})", formatted.join(" "))
}

/// Format a list across multiple lines
fn format_list_multiline(
    elements: &[&Value],
    indent: usize,
    config: &FormatterConfig,
    symbol_table: &SymbolTable,
) -> String {
    if elements.is_empty() {
        return "()".to_string();
    }

    let new_indent = indent + config.indent_width;
    let indent_str = " ".repeat(new_indent);

    let mut result = String::from("(");

    // Format first element on the same line as opening paren if it's short
    let first_formatted = format_value(elements[0], new_indent, config, symbol_table);
    result.push_str(&first_formatted);

    // Format remaining elements on new lines
    for element in &elements[1..] {
        result.push('\n');
        result.push_str(&indent_str);
        let formatted = format_value(element, new_indent, config, symbol_table);
        result.push_str(&formatted);
    }

    result.push(')');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_simple_number() {
        let config = FormatterConfig::default();
        let formatted = format_code("42", &config).unwrap();
        assert_eq!(formatted, "42");
    }

    #[test]
    fn test_format_simple_list() {
        let config = FormatterConfig::default();
        let formatted = format_code("(+ 1 2)", &config).unwrap();
        // Should be formatted, may be on one or multiple lines
        assert!(formatted.contains('('));
        assert!(formatted.contains(')'));
    }

    #[test]
    fn test_format_nil() {
        let config = FormatterConfig::default();
        let formatted = format_code("nil", &config).unwrap();
        assert_eq!(formatted, "nil");
    }

    #[test]
    fn test_format_boolean() {
        let config = FormatterConfig::default();
        let formatted_true = format_code("#t", &config).unwrap();
        let formatted_false = format_code("#f", &config).unwrap();
        assert_eq!(formatted_true, "#t");
        assert_eq!(formatted_false, "#f");
    }

    #[test]
    fn test_format_string() {
        let config = FormatterConfig::default();
        let formatted = format_code("\"hello\"", &config).unwrap();
        assert!(formatted.contains("hello"));
    }

    #[test]
    fn test_format_vector() {
        let config = FormatterConfig::default();
        let formatted = format_code("[1 2 3]", &config).unwrap();
        assert!(formatted.contains('['));
        assert!(formatted.contains(']'));
    }
}
