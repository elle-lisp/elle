//! Core formatting logic
//!
//! Implements a recursive pretty-printer for s-expressions.

use super::config::FormatterConfig;
use crate::reader::Lexer;
use crate::symbol::SymbolTable;
use crate::value::{SymbolId, Value};
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
    use crate::value::heap::{deref, HeapObject};

    if value.is_nil() {
        return "nil".to_string();
    }

    if let Some(b) = value.as_bool() {
        return if b { "#t" } else { "#f" }.to_string();
    }

    if let Some(n) = value.as_int() {
        return n.to_string();
    }

    if let Some(n) = value.as_float() {
        return n.to_string();
    }

    if let Some(id) = value.as_symbol() {
        let sym_id = SymbolId(id);
        return symbol_table
            .name(sym_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("#{}", id));
    }

    if let Some(name) = value.as_keyword_name() {
        return format!(":{}", name);
    }

    // Handle heap values
    if let Some(_ptr) = value.as_heap_ptr() {
        let obj = unsafe { deref(*value) };
        match obj {
            HeapObject::String(s) => {
                return format!("\"{}\"", s.escape_default());
            }
            HeapObject::Array(v) => {
                if let Ok(elements) = v.try_borrow() {
                    if elements.is_empty() {
                        return "[]".to_string();
                    } else {
                        let items: Vec<String> = elements
                            .iter()
                            .map(|e| format_value(e, indent, config, symbol_table))
                            .collect();
                        return format!("[{}]", items.join(" "));
                    }
                }
                return "[<borrowed>]".to_string();
            }
            HeapObject::Cons(cons) => {
                return format_cons(&cons.first, &cons.rest, indent, config, symbol_table);
            }
            HeapObject::Table(_) => {
                // For Phase 1, just return a placeholder
                return "{{...}}".to_string();
            }
            HeapObject::Struct(_) => {
                // For Phase 1, just return a placeholder
                return "{{...}}".to_string();
            }
            HeapObject::Closure(_) => return "#<closure>".to_string(),
            HeapObject::NativeFn(_) => return "#<native-fn>".to_string(),
            HeapObject::LibHandle(_) => return "#<lib-handle>".to_string(),
            HeapObject::CHandle(_, _) => return "#<c-handle>".to_string(),
            HeapObject::Tuple(elems) => {
                let items: Vec<String> = elems
                    .iter()
                    .map(|e| format_value(e, indent, config, symbol_table))
                    .collect();
                return format!("[{}]", items.join(" "));
            }
            HeapObject::ThreadHandle(_) => return "#<thread-handle>".to_string(),
            HeapObject::Cell(_, _) => return "#<cell>".to_string(),
            HeapObject::Float(_) => return "#<float>".to_string(),
            HeapObject::Fiber(_) => return "#<fiber>".to_string(),
            HeapObject::Syntax(s) => return format!("#<syntax:{}>", s),
            HeapObject::Binding(_) => return "#<binding>".to_string(),
        }
    }

    // Fallback for unknown types
    "#<unknown>".to_string()
}

/// Format a cons cell (list)
fn format_cons(
    head: &Value,
    tail: &Value,
    indent: usize,
    config: &FormatterConfig,
    symbol_table: &SymbolTable,
) -> String {
    use crate::value::heap::{deref, HeapObject};

    // Collect all elements in the list
    let mut elements = vec![head];
    let mut current = tail;

    loop {
        if current.is_nil() || current.is_empty_list() {
            break;
        }

        if let Some(_ptr) = current.as_heap_ptr() {
            let obj = unsafe { deref(*current) };
            if let HeapObject::Cons(cons) = obj {
                elements.push(&cons.first);
                current = &cons.rest;
                continue;
            }
        }

        // Improper list - not common in well-formed Elle code
        elements.push(current);
        break;
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
