//! Display and Debug implementations for values
//!
//! This module contains the Display and Debug trait implementations
//! for the NaN-boxed Value type, providing human-readable representations
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
                write!(f, "{}", name)
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
            return write!(f, "<unknown:{:#x}>", self.to_bits());
        }

        // Cons cell (list)
        if let Some(_cons) = self.as_cons() {
            return self.fmt_cons(f);
        }

        // Array
        if let Some(vec_ref) = self.as_array() {
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
        if let Some(table_ref) = self.as_table() {
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
                write!(f, "{} {}", k, v)?;
            }
            return write!(f, "}}");
        }

        // Closure
        if self.is_closure() {
            return write!(f, "<closure>");
        }

        // Cell
        if let Some(cell_ref) = self.as_cell() {
            let val = cell_ref.borrow();
            return write!(f, "<cell {}>", val);
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

        // Binding
        if self.is_binding() {
            return write!(f, "#<binding>");
        }

        // Buffer
        if let Some(buf_ref) = self.as_buffer() {
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

        // Blob (mutable binary data)
        if let Some(blob_ref) = self.as_blob() {
            let borrowed = blob_ref.borrow();
            write!(f, "#blob[")?;
            for (i, byte) in borrowed.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:02x}", byte)?;
            }
            return write!(f, "]");
        }

        // Tuple
        if let Some(elems) = self.as_tuple() {
            write!(f, "[")?;
            for (i, v) in elems.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", v)?;
            }
            return write!(f, "]");
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

        // Default for unknown heap types
        write!(f, "<heap:{:#x}>", self.to_bits() & 0x0000_FFFF_FFFF_FFFF)
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
                write!(f, "{}", name)
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
        // SSO string (not heap) — quoted
        if self.is_string() {
            return self
                .with_string(|s| write!(f, "\"{}\"", s))
                .unwrap_or(Ok(()));
        }
        if !self.is_heap() {
            return write!(f, "<unknown:{:#x}>", self.to_bits());
        }
        // Cons cell — use Debug recursively
        if self.as_cons().is_some() {
            return self.fmt_cons_debug(f);
        }
        // Array
        if let Some(vec_ref) = self.as_array() {
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
        // Buffer
        if let Some(buf_ref) = self.as_buffer() {
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
        // Blob (mutable binary data)
        if let Some(blob_ref) = self.as_blob() {
            let borrowed = blob_ref.borrow();
            write!(f, "#blob[")?;
            for (i, byte) in borrowed.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:02x}", byte)?;
            }
            return write!(f, "]");
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
