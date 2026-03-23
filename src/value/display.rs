//! Display and Debug implementations for values
//!
//! This module contains the Display and Debug trait implementations
//! for the tagged-union Value type, providing human-readable representations
//! of values for debugging and user output.

use crate::value::Value;
use std::fmt;

/// Resolve a symbol ID to its name via the thread-local symbol table.
fn resolve_symbol(id: u32) -> Option<String> {
    crate::context::resolve_symbol_name(id)
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Handle immediate values
        if self.is_nil() {
            return write!(f, "nil");
        }

        if self.is_empty_list() {
            return write!(f, "()");
        }

        if self.is_undefined() {
            return write!(f, "#<undefined>");
        }

        if let Some(b) = self.as_bool() {
            return write!(f, "{}", b);
        }

        if let Some(n) = self.as_int() {
            return write!(f, "{}", n);
        }

        if let Some(n) = self.as_float() {
            return write!(f, "{}", n);
        }

        if let Some(id) = self.as_symbol() {
            return if let Some(name) = resolve_symbol(id) {
                write!(f, "'{}", name)
            } else {
                write!(f, "#<sym:{}>", id)
            };
        }

        if let Some(name) = self.as_keyword_name() {
            return write!(f, ":{}", name);
        }

        if let Some(addr) = self.as_pointer() {
            return write!(f, "<pointer 0x{:x}>", addr);
        }

        // SSO string (not heap)
        if self.is_string() {
            return self.with_string(|s| write!(f, "{}", s)).unwrap_or(Ok(()));
        }

        // Handle heap values
        if !self.is_heap() {
            return write!(
                f,
                "<unknown:tag={:#x},payload={:#x}>",
                self.tag, self.payload
            );
        }

        // Cons cell (list)
        if let Some(_cons) = self.as_cons() {
            return self.fmt_cons(f);
        }

        // Array
        if let Some(vec_ref) = self.as_array_mut() {
            let vec = vec_ref.borrow();
            write!(f, "@[")?;
            for (i, v) in vec.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", v)?;
            }
            return write!(f, "]");
        }

        // Table
        if let Some(table_ref) = self.as_struct_mut() {
            let table = table_ref.borrow();
            write!(f, "@{{")?;
            let mut first = true;
            for (k, v) in table.iter() {
                if !first {
                    write!(f, " ")?;
                }
                first = false;
                write!(f, "{} {}", k, v)?;
            }
            return write!(f, "}}");
        }

        // Struct
        if let Some(struct_map) = self.as_struct() {
            write!(f, "{{")?;
            let mut first = true;
            for (k, v) in struct_map.iter() {
                if !first {
                    write!(f, " ")?;
                }
                first = false;
                write!(f, "{} {:?}", k, v)?;
            }
            return write!(f, "}}");
        }

        // Closure
        if self.is_closure() {
            return write!(f, "<closure>");
        }

        // Box
        if let Some(cell_ref) = self.as_lbox() {
            let val = cell_ref.borrow();
            return write!(f, "<box {}>", val);
        }

        // Fiber
        if let Some(handle) = self.as_fiber() {
            return match handle.try_with(|fib| fib.status.as_str()) {
                Some(status) => write!(f, "<fiber:{}>", status),
                None => write!(f, "<fiber:taken>"),
            };
        }

        // Managed pointer
        if let Some(cell) = self.as_managed_pointer() {
            return match cell.get() {
                Some(addr) => write!(f, "<pointer 0x{:x}>", addr),
                None => write!(f, "<freed-pointer>"),
            };
        }

        // Syntax object
        if let Some(s) = self.as_syntax() {
            return write!(f, "#<syntax:{}>", s.as_ref());
        }

        // Parameter
        if let Some((id, _)) = self.as_parameter() {
            return write!(f, "<parameter:{}>", id);
        }

        // @string
        if let Some(buf_ref) = self.as_string_mut() {
            let borrowed = buf_ref.borrow();
            write!(f, "@\"")?;
            // Display as UTF-8 where valid, escape otherwise
            for &byte in borrowed.iter() {
                if byte == b'"' {
                    write!(f, "\\\"")?;
                } else if byte == b'\\' {
                    write!(f, "\\\\")?;
                } else if (0x20..0x7f).contains(&byte) {
                    write!(f, "{}", byte as char)?;
                } else {
                    write!(f, "\\x{:02x}", byte)?;
                }
            }
            return write!(f, "\"");
        }

        // Bytes (immutable binary data)
        if let Some(b) = self.as_bytes() {
            write!(f, "#bytes[")?;
            for (i, byte) in b.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:02x}", byte)?;
            }
            return write!(f, "]");
        }

        // @bytes (mutable binary data)
        if let Some(blob_ref) = self.as_bytes_mut() {
            let borrowed = blob_ref.borrow();
            write!(f, "#@bytes[")?;
            for (i, byte) in borrowed.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:02x}", byte)?;
            }
            return write!(f, "]");
        }

        // Array (immutable)
        if let Some(elems) = self.as_array() {
            write!(f, "[")?;
            for (i, v) in elems.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", v)?;
            }
            return write!(f, "]");
        }

        // Set (immutable)
        if let Some(set) = self.as_set() {
            write!(f, "|")?;
            for (i, v) in set.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", v)?;
            }
            return write!(f, "|");
        }

        // Set (mutable)
        if let Some(set_ref) = self.as_set_mut() {
            let set = set_ref.borrow();
            write!(f, "@|")?;
            for (i, v) in set.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", v)?;
            }
            return write!(f, "|");
        }

        // FFI signature
        if self.as_ffi_signature().is_some() {
            return write!(f, "<ffi-signature>");
        }

        // FFI type descriptor
        if let Some(desc) = self.as_ffi_type() {
            return match desc {
                crate::ffi::types::TypeDesc::Struct(sd) if sd.fields.len() <= 5 => {
                    let names: Vec<String> = sd.fields.iter().map(|f| f.short_name()).collect();
                    write!(f, "<ffi-type:struct({})>", names.join(", "))
                }
                _ => write!(f, "<ffi-type:{}>", desc.short_name()),
            };
        }

        // Library handle
        if let Some(id) = self.as_lib_handle() {
            return write!(f, "<lib-handle:{}>", id);
        }

        // External object — delegate to type-specific Display if available
        if let Some(port) = self.as_external::<crate::port::Port>() {
            return write!(f, "{}", port);
        }
        if let Some(name) = self.external_type_name() {
            return write!(f, "#<{}>", name);
        }

        // Default for unknown heap types
        write!(f, "<heap:{:#x}>", self.payload)
    }
}

impl fmt::Debug for Value {
    /// Machine-readable representation. Strings are quoted, bools are true/false.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_nil() {
            return write!(f, "nil");
        }
        if self.is_empty_list() {
            return write!(f, "()");
        }
        if self.is_undefined() {
            return write!(f, "#<undefined>");
        }
        if let Some(b) = self.as_bool() {
            return write!(f, "{}", if b { "true" } else { "false" });
        }
        if let Some(n) = self.as_int() {
            return write!(f, "{}", n);
        }
        if let Some(n) = self.as_float() {
            return write!(f, "{}", n);
        }
        if let Some(id) = self.as_symbol() {
            return if let Some(name) = resolve_symbol(id) {
                write!(f, "'{}", name)
            } else {
                write!(f, "#<sym:{}>", id)
            };
        }
        if let Some(name) = self.as_keyword_name() {
            return write!(f, ":{}", name);
        }
        if let Some(addr) = self.as_pointer() {
            return write!(f, "<pointer 0x{:x}>", addr);
        }
        // SSO string or heap LString — quoted with escaping
        if self.is_string() {
            return self
                .with_string(|s| {
                    write!(f, "\"")?;
                    for ch in s.chars() {
                        match ch {
                            '\\' => write!(f, "\\\\")?,
                            '"' => write!(f, "\\\"")?,
                            c => write!(f, "{}", c)?,
                        }
                    }
                    write!(f, "\"")
                })
                .unwrap_or(Ok(()));
        }
        if !self.is_heap() {
            return write!(
                f,
                "<unknown:tag={:#x},payload={:#x}>",
                self.tag, self.payload
            );
        }
        // Cons cell — use Debug recursively
        if self.as_cons().is_some() {
            return self.fmt_cons_debug(f);
        }
        // Array
        if let Some(vec_ref) = self.as_array_mut() {
            let vec = vec_ref.borrow();
            write!(f, "@[")?;
            for (i, v) in vec.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:?}", v)?;
            }
            return write!(f, "]");
        }
        // @string
        if let Some(buf_ref) = self.as_string_mut() {
            let borrowed = buf_ref.borrow();
            write!(f, "@\"")?;
            for &byte in borrowed.iter() {
                if byte == b'"' {
                    write!(f, "\\\"")?;
                } else if byte == b'\\' {
                    write!(f, "\\\\")?;
                } else if (0x20..0x7f).contains(&byte) {
                    write!(f, "{}", byte as char)?;
                } else {
                    write!(f, "\\x{:02x}", byte)?;
                }
            }
            return write!(f, "\"");
        }
        // Bytes (immutable binary data)
        if let Some(b) = self.as_bytes() {
            write!(f, "#bytes[")?;
            for (i, byte) in b.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:02x}", byte)?;
            }
            return write!(f, "]");
        }
        // @bytes (mutable binary data)
        if let Some(blob_ref) = self.as_bytes_mut() {
            let borrowed = blob_ref.borrow();
            write!(f, "#@bytes[")?;
            for (i, byte) in borrowed.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:02x}", byte)?;
            }
            return write!(f, "]");
        }
        // Array (immutable)
        if let Some(elems) = self.as_array() {
            write!(f, "[")?;
            for (i, v) in elems.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:?}", v)?;
            }
            return write!(f, "]");
        }
        // Set (immutable)
        if let Some(set) = self.as_set() {
            write!(f, "|")?;
            for (i, v) in set.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:?}", v)?;
            }
            return write!(f, "|");
        }
        // Set (mutable)
        if let Some(set_ref) = self.as_set_mut() {
            let set = set_ref.borrow();
            write!(f, "@|")?;
            for (i, v) in set.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:?}", v)?;
            }
            return write!(f, "|");
        }
        // Struct (immutable) — use Debug for keys and values
        if let Some(struct_map) = self.as_struct() {
            write!(f, "{{")?;
            let mut first = true;
            for (k, v) in struct_map.iter() {
                if !first {
                    write!(f, " ")?;
                }
                first = false;
                write!(f, "{:?} {:?}", k, v)?;
            }
            return write!(f, "}}");
        }
        // Struct (mutable) — use Debug for keys and values
        if let Some(table_ref) = self.as_struct_mut() {
            let table = table_ref.borrow();
            write!(f, "@{{")?;
            let mut first = true;
            for (k, v) in table.iter() {
                if !first {
                    write!(f, " ")?;
                }
                first = false;
                write!(f, "{:?} {:?}", k, v)?;
            }
            return write!(f, "}}");
        }
        // Everything else — delegate to Display
        write!(f, "{}", self)
    }
}

impl Value {
    /// Format a cons cell (list) with Debug (quoted strings)
    fn fmt_cons_debug(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        let mut current = *self;
        let mut first = true;
        loop {
            if current.is_nil() || current.is_empty_list() {
                break;
            }
            if !first {
                write!(f, " ")?;
            }
            first = false;
            if let Some(c) = current.as_cons() {
                write!(f, "{:?}", c.first)?;
                current = c.rest;
            } else {
                write!(f, ". {:?}", current)?;
                break;
            }
        }
        write!(f, ")")
    }

    /// Format a cons cell (list) with proper list notation
    fn fmt_cons(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;

        let mut current = *self;
        let mut first = true;

        loop {
            if current.is_nil() || current.is_empty_list() {
                break;
            }

            if !first {
                write!(f, " ")?;
            }
            first = false;

            if let Some(c) = current.as_cons() {
                write!(f, "{}", c.first)?;
                current = c.rest;
            } else {
                // Improper list: (a . b)
                write!(f, ". {}", current)?;
                break;
            }
        }

        write!(f, ")")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::error::error_val;

    /// Debug repr of a string containing a double-quote must escape it.
    #[test]
    fn test_debug_string_escapes_double_quote() {
        let val = Value::string("say \"hello\"");
        let repr = format!("{:?}", val);
        assert_eq!(repr, r#""say \"hello\"""#);
    }

    /// Debug repr of a string containing a backslash must escape it.
    #[test]
    fn test_debug_string_escapes_backslash() {
        let val = Value::string("path\\to\\file");
        let repr = format!("{:?}", val);
        assert_eq!(repr, r#""path\\to\\file""#);
    }

    /// Debug repr of a string containing both backslash and double-quote.
    /// Backslash must be escaped before quote (order matters).
    #[test]
    fn test_debug_string_escapes_backslash_and_quote() {
        let val = Value::string("a\\\"b");
        let repr = format!("{:?}", val);
        assert_eq!(repr, r#""a\\\"b""#);
    }

    /// Display of a struct with a string value must quote and escape the string.
    #[test]
    fn test_display_struct_quotes_string_values() {
        let err = error_val("type-error", "expected \"integer\"");
        let repr = format!("{}", err);
        // The struct has :error and :message keys (BTreeMap, sorted by key).
        // :error → :type-error (keyword, no quotes)
        // :message → "expected \"integer\"" (string, quoted and escaped)
        assert!(repr.contains(r#""expected \"integer\"""#), "got: {}", repr);
    }
}
