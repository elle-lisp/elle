//! Display and Debug implementations for values
//!
//! This module contains the Display and Debug trait implementations
//! for the NaN-boxed Value type, providing human-readable representations
//! of values for debugging and user output.

use crate::value::Value;
use std::fmt;

/// Resolve a symbol ID to its name via the thread-local symbol table.
fn resolve_symbol(id: u32) -> Option<String> {
    crate::ffi::primitives::context::resolve_symbol_name(id)
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

        // Handle heap values
        if !self.is_heap() {
            return write!(f, "<unknown:{:#x}>", self.to_bits());
        }

        // String
        if let Some(s) = self.as_string() {
            return write!(f, "{}", s);
        }

        // Cons cell (list)
        if let Some(_cons) = self.as_cons() {
            return self.fmt_cons(f);
        }

        // Array
        if let Some(vec_ref) = self.as_array() {
            let vec = vec_ref.borrow();
            write!(f, "[")?;
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
            write!(f, "{{")?;
            let mut first = true;
            for (k, v) in table.iter() {
                if !first {
                    write!(f, " ")?;
                }
                first = false;
                write!(f, "{:?} {}", k, v)?;
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
                write!(f, "{:?} {}", k, v)?;
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

        // Syntax object
        if let Some(s) = self.as_syntax() {
            return write!(f, "#<syntax:{}>", s.as_ref());
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

        // Default for unknown heap types
        write!(f, "<heap:{:#x}>", self.to_bits() & 0x0000_FFFF_FFFF_FFFF)
    }
}

impl fmt::Debug for Value {
    /// Machine-readable representation. Strings are quoted, bools are #t/#f.
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
            return write!(f, "{}", if b { "#t" } else { "#f" });
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
        if !self.is_heap() {
            return write!(f, "<unknown:{:#x}>", self.to_bits());
        }
        // String — quoted
        if let Some(s) = self.as_string() {
            return write!(f, "\"{}\"", s);
        }
        // Cons cell — use Debug recursively
        if self.as_cons().is_some() {
            return self.fmt_cons_debug(f);
        }
        // Array
        if let Some(vec_ref) = self.as_array() {
            let vec = vec_ref.borrow();
            write!(f, "[")?;
            for (i, v) in vec.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{:?}", v)?;
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
