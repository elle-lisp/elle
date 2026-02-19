//! Display and Debug implementations for values
//!
//! This module contains the Display and Debug trait implementations
//! for the NaN-boxed Value type, providing human-readable representations
//! of values for debugging and user output.

use crate::value::Value;
use std::fmt;

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Handle immediate values
        if self.is_nil() {
            return write!(f, "nil");
        }

        if self.is_empty_list() {
            return write!(f, "()");
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
            return write!(f, "{}", id);
        }

        if let Some(id) = self.as_keyword() {
            return write!(f, ":{}", id);
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

        // Vector
        if let Some(vec_ref) = self.as_vector() {
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

        // Coroutine
        if self.is_coroutine() {
            return write!(f, "<coroutine>");
        }

        // Condition
        if let Some(_cond) = self.as_condition() {
            return write!(f, "<condition>");
        }

        // Default for unknown heap types
        write!(f, "<heap:{:#x}>", self.to_bits() & 0x0000_FFFF_FFFF_FFFF)
    }
}

impl Value {
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
