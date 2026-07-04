use super::*;

impl VM {
    /// Format a runtime error value with source location.
    pub(crate) fn format_error_with_location(&self, err_value: Value) -> String {
        let mut result = String::new();

        // Stack trace first (shallowest frame first, drilling down to error origin)
        let trace = self.capture_stack_trace();
        if !trace.is_empty() {
            const MAX_TRACE_DEPTH: usize = 20;
            for frame in trace.iter().rev().take(MAX_TRACE_DEPTH) {
                if let Some(name) = &frame.function_name {
                    result.push_str(&format!("  in {}", name));
                    if let Some(loc) = &frame.location {
                        result.push_str(&format!(" at {}", loc));
                    }
                    result.push('\n');
                }
            }
            if trace.len() > MAX_TRACE_DEPTH {
                result.push_str(&format!(
                    "  ... {} more frames\n",
                    trace.len() - MAX_TRACE_DEPTH
                ));
            }
        }

        // Error location and source context
        if let Some(loc) = &self.error_loc {
            result.push_str(&format!("  at {}\n", loc));

            // Add source context if available
            if let Some(source) = crate::error::formatting::load_source_for_loc(loc) {
                if let Some(line) = crate::error::formatting::extract_source_line(&source, loc.line)
                {
                    let truncated = if line.len() > 120 {
                        format!("{}...", &line[..117])
                    } else {
                        line.to_string()
                    };
                    result.push_str(&format!("   {}\n", truncated));

                    let caret = crate::error::formatting::highlight_column(&line, loc.col);
                    result.push_str(&format!("   {}\n", caret));
                }
            }
        }

        // Error value last. Render through this instance's symbol table so a
        // symbol-bearing error value shows names (`'name`), not `#<sym:id>` — a
        // bare `{:?}` has no table (docs/impl/region-ctx.md § "Symbols").
        result.push_str(&format!(
            "✗ Runtime error: {}",
            err_value.debug_with(self.symbols().as_deref())
        ));

        result
    }

    /// Capture current call stack as trace frames
    pub fn capture_stack_trace(&self) -> Vec<StackFrame> {
        self.fiber
            .call_stack
            .iter()
            .rev()
            .map(|frame| {
                let location = frame.location_map.get(&frame.ip).cloned();
                StackFrame {
                    function_name: Some(frame.name.to_string()),
                    location,
                }
            })
            .collect()
    }

    /// Wrap a string error with stack trace information
    pub fn wrap_error(&self, error: String) -> String {
        let trace = self.capture_stack_trace();
        if trace.is_empty() {
            return error;
        }

        let mut result = error;
        for frame in &trace {
            result.push_str("\n    in ");
            if let Some(ref name) = frame.function_name {
                result.push_str(name);
            } else {
                result.push_str("<anonymous>");
            }
            if let Some(ref loc) = frame.location {
                result.push_str(&format!(" at {}", loc));
            }
        }
        result
    }
}
