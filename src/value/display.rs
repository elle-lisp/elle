//! Display and Debug implementations for values
//!
//! The tagged-union `Value` renders through one body, `fmt_value`, parameterized
//! by a `debug` flag and an optional `&SymbolTable`. Symbol-name resolution is
//! per-instance (docs/impl/region/ctx.md § "Symbols through the ctx"): a bare
//! `Display`/`Debug` carries no table and renders a symbol as `#<sym:id>`, while
//! [`Value::display_with`] / [`Value::debug_with`] thread an instance's table so
//! names resolve all the way down.

use crate::symbol::SymbolTable;
use crate::value::cycle::{fmt_enter, HareState};
use crate::value::Value;
use std::fmt;

/// Format a `@string` (mutable string buffer) value.
fn fmt_string_mut(buf: &std::cell::RefCell<Vec<u8>>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let borrowed = buf.borrow();
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
    write!(f, "\"")
}

/// Format immutable `bytes` value.
fn fmt_bytes(b: &[u8], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "b[")?;
    for (i, byte) in b.iter().enumerate() {
        if i > 0 {
            write!(f, " ")?;
        }
        write!(f, "{}", byte)?;
    }
    write!(f, "]")
}

/// Format mutable `@bytes` value.
fn fmt_bytes_mut(blob: &std::cell::RefCell<Vec<u8>>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let borrowed = blob.borrow();
    write!(f, "@b[")?;
    for (i, byte) in borrowed.iter().enumerate() {
        if i > 0 {
            write!(f, " ")?;
        }
        write!(f, "{}", byte)?;
    }
    write!(f, "]")
}

/// Format a float, ensuring whole numbers display with a trailing `.0`
/// so that `3.0` prints as `3.0`, not `3`.
fn write_float(f: &mut fmt::Formatter<'_>, n: f64) -> fmt::Result {
    if n.is_infinite() {
        return if n.is_sign_positive() {
            write!(f, "inf")
        } else {
            write!(f, "-inf")
        };
    }
    if n.is_nan() {
        return write!(f, "NaN");
    }
    if n.fract() == 0.0 {
        write!(f, "{:.1}", n)
    } else {
        write!(f, "{}", n)
    }
}

/// Format a `Value`, optionally resolving symbol names through `symbols`. The one
/// rendering body for both `Display` (`debug == false`) and `Debug`
/// (`debug == true`); it recurses by calling itself, so a threaded table reaches
/// every nested symbol. `symbols` is `None` for a bare trait render — a symbol
/// then prints `#<sym:id>` — and `Some` when a caller threads its instance's
/// table via [`Value::display_with`] / [`Value::debug_with`]
/// (docs/impl/region/ctx.md § "Symbols through the ctx").
///
/// Where the two renderings diverge they branch on `debug`: strings (Debug quotes
/// and escapes), and the element/key recursion of cons/array/set/struct. A struct
/// value is always rendered in Debug style (matching the historical
/// `"{} {:?}"`/`"{:?} {:?}"`); a box's contents are always rendered in Display
/// style (matching the historical Debug-delegates-to-Display for boxes).
pub(crate) fn fmt_value(
    v: &Value,
    symbols: Option<&SymbolTable>,
    debug: bool,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    // Immediates shared verbatim by both renderings.
    if v.is_nil() {
        return write!(f, "nil");
    }
    if v.is_empty_list() {
        return write!(f, "()");
    }
    if v.is_undefined() {
        return write!(f, "#<undefined>");
    }
    if let Some(b) = v.as_bool() {
        return write!(f, "{}", b);
    }
    if let Some(n) = v.as_int() {
        return write!(f, "{}", n);
    }
    if let Some(n) = v.as_float() {
        return write_float(f, n);
    }
    if let Some(id) = v.as_symbol() {
        // Identical in Display and Debug: the bare name if the threaded table
        // resolves it, else `#<sym:id>`. The name carries no leading `'` —
        // Scheme/CL print a symbol as its bare name (`'` is reader syntax for
        // `quote`, not part of a symbol's printed form), and this matches the
        // `string`/`println` path (`src/primitives/convert.rs`), so a list of
        // symbols renders the same `(a b c)` everywhere it appears (a bare
        // `Display`, a struct field, an error's `:syntax` form). A symbol stays
        // distinguishable from a `"string"` in Debug mode (strings quote) and
        // from a `:keyword` always.
        return match symbols.and_then(|s| s.name(crate::value::SymbolId(id))) {
            Some(name) => write!(f, "{}", name),
            None => write!(f, "#<sym:{}>", id),
        };
    }
    if let Some(name) = v.as_keyword_name() {
        return write!(f, ":{}", name);
    }
    if let Some(addr) = v.as_pointer() {
        return write!(f, "<pointer 0x{:x}>", addr);
    }
    // Native-fn is an immediate (tag below the heap boundary) — never deref.
    if v.is_native_fn() {
        return write!(f, "<native-fn>");
    }
    // String (SSO or heap): Debug quotes and escapes; Display prints raw.
    if v.is_string() {
        return v
            .with_string(|s| {
                if debug {
                    write!(f, "\"")?;
                    for ch in s.chars() {
                        match ch {
                            '\\' => write!(f, "\\\\")?,
                            '"' => write!(f, "\\\"")?,
                            c => write!(f, "{}", c)?,
                        }
                    }
                    write!(f, "\"")
                } else {
                    write!(f, "{}", s)
                }
            })
            .unwrap_or(Ok(()));
    }

    // Handle heap values.
    if !v.is_heap() {
        return write!(f, "<unknown:tag={:#x},payload={:#x}>", v.tag, v.payload);
    }

    // Pair cell (list).
    if v.as_pair().is_some() {
        return fmt_cons(v, symbols, debug, f);
    }

    // Array (@array, mutable).
    if let Some(vec_ref) = v.as_array_mut_raw() {
        let _guard = match fmt_enter(v.payload as usize) {
            Some(g) => g,
            None => return write!(f, "@[<cycle>]"),
        };
        let vec = vec_ref.borrow();
        write!(f, "@[")?;
        for (i, item) in vec.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            fmt_value(item, symbols, debug, f)?;
        }
        return write!(f, "]");
    }

    // @struct (mutable). The key follows the outer mode; the value is always
    // rendered Debug-style.
    if let Some(table_ref) = v.as_struct_mut_raw() {
        let _guard = match fmt_enter(v.payload as usize) {
            Some(g) => g,
            None => return write!(f, "@{{<cycle>}}"),
        };
        let table = table_ref.borrow();
        write!(f, "@{{")?;
        let mut first = true;
        for (k, val) in table.iter() {
            if !first {
                write!(f, " ")?;
            }
            first = false;
            crate::value::types::fmt_table_key(k, symbols, debug, f)?;
            write!(f, " ")?;
            fmt_value(val, symbols, true, f)?;
        }
        return write!(f, "}}");
    }

    // struct (immutable).
    if let Some(struct_map) = v.as_struct() {
        write!(f, "{{")?;
        let mut first = true;
        for (k, val) in struct_map.iter() {
            if !first {
                write!(f, " ")?;
            }
            first = false;
            crate::value::types::fmt_table_key(k, symbols, debug, f)?;
            write!(f, " ")?;
            fmt_value(val, symbols, true, f)?;
        }
        return write!(f, "}}");
    }

    // Closure.
    if v.is_closure() {
        return write!(f, "<closure>");
    }

    // Box — inner always rendered in Display mode (the original Debug impl had no
    // box arm and fell through to Display).
    if let Some(cell_ref) = v.as_lbox_raw() {
        let _guard = match fmt_enter(v.payload as usize) {
            Some(g) => g,
            None => return write!(f, "<box <cycle>>"),
        };
        let val = cell_ref.borrow();
        write!(f, "<box ")?;
        fmt_value(&val, symbols, false, f)?;
        return write!(f, ">");
    }

    // Fiber.
    if let Some(handle) = v.as_fiber() {
        return match handle.try_with(|fib| fib.status.as_str()) {
            Some(status) => write!(f, "<fiber:{}>", status),
            None => write!(f, "<fiber:taken>"),
        };
    }

    // Managed pointer.
    if let Some(cell) = v.as_managed_pointer() {
        return match cell.get() {
            Some(addr) => write!(f, "<pointer 0x{:x}>", addr),
            None => write!(f, "<freed-pointer>"),
        };
    }

    // Syntax object.
    if let Some(s) = v.as_syntax() {
        return write!(f, "#<syntax:{}>", s);
    }

    // Parameter.
    if let Some((id, _)) = v.as_parameter() {
        return write!(f, "<parameter:{}>", id);
    }

    if let Some(buf_ref) = v.as_string_mut() {
        return fmt_string_mut(buf_ref, f);
    }
    if let Some(b) = v.as_bytes() {
        return fmt_bytes(b, f);
    }
    if let Some(blob_ref) = v.as_bytes_mut() {
        return fmt_bytes_mut(blob_ref, f);
    }

    // Array (immutable).
    if let Some(elems) = v.as_array() {
        write!(f, "[")?;
        for (i, item) in elems.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            fmt_value(item, symbols, debug, f)?;
        }
        return write!(f, "]");
    }

    // Set (immutable).
    if let Some(set) = v.as_set() {
        write!(f, "|")?;
        for (i, item) in set.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            fmt_value(item, symbols, debug, f)?;
        }
        return write!(f, "|");
    }

    // Set (mutable).
    if let Some(set_ref) = v.as_set_mut_raw() {
        let _guard = match fmt_enter(v.payload as usize) {
            Some(g) => g,
            None => return write!(f, "@|<cycle>|"),
        };
        let set = set_ref.borrow();
        write!(f, "@|")?;
        for (i, item) in set.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            fmt_value(item, symbols, debug, f)?;
        }
        return write!(f, "|");
    }

    // FFI signature.
    if v.as_ffi_signature().is_some() {
        return write!(f, "<ffi-signature>");
    }

    // FFI type descriptor.
    if let Some(desc) = v.as_ffi_type() {
        return match desc {
            crate::ffi::types::TypeDesc::Struct(sd) if sd.fields.len() <= 5 => {
                let names: Vec<String> = sd.fields.iter().map(|fld| fld.short_name()).collect();
                write!(f, "<ffi-type:struct({})>", names.join(", "))
            }
            _ => write!(f, "<ffi-type:{}>", desc.short_name()),
        };
    }

    // Library handle.
    if let Some(id) = v.as_lib_handle() {
        return write!(f, "<lib-handle:{}>", id);
    }

    // External object — delegate to type-specific Display if available.
    if let Some(port) = v.as_external::<crate::port::Port>() {
        return write!(f, "{}", port);
    }
    if let Some(name) = v.external_type_name() {
        return write!(f, "#<{}>", name);
    }

    // Default for unknown heap types.
    write!(f, "<heap:{:#x}>", v.payload)
}

/// Format a cons cell (list) with cycle detection, threading `symbols` and the
/// `debug` mode to every element. `debug` selects element rendering: `{:?}` vs
/// `{}` in the original two impls.
fn fmt_cons(
    v: &Value,
    symbols: Option<&SymbolTable>,
    debug: bool,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    write!(f, "(")?;
    let mut current = *v;
    let mut hare = HareState::new(*v);
    let mut first = true;
    loop {
        if current.is_nil() || current.is_empty_list() {
            break;
        }
        if !first {
            write!(f, " ")?;
        }
        first = false;
        if let Some(c) = current.as_pair() {
            fmt_value(&c.first, symbols, debug, f)?;
            current = c.rest;
            if current.is_heap() && hare.advance(current) {
                write!(f, " . <cycle>")?;
                break;
            }
        } else {
            write!(f, ". ")?;
            fmt_value(&current, symbols, debug, f)?;
            break;
        }
    }
    write!(f, ")")
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_value(self, None, false, f)
    }
}

impl fmt::Debug for Value {
    /// Machine-readable representation. Strings are quoted, bools are true/false.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_value(self, None, true, f)
    }
}

/// A `Value` paired with a symbol table for name-resolving rendering. Returned by
/// [`Value::display_with`]; its `Display` resolves symbol names (the bare `name`)
/// through the table, where a bare `Display`/`Debug` (no table) would print
/// `#<sym:id>`.
pub struct DisplayWith<'a> {
    value: Value,
    symbols: Option<&'a SymbolTable>,
}

impl fmt::Display for DisplayWith<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_value(&self.value, self.symbols, false, f)
    }
}

/// Like [`DisplayWith`] but renders in `Debug` style (quoted strings, etc.).
/// Returned by [`Value::debug_with`]. It implements `Display` (not `Debug`) so
/// `format!("{}", v.debug_with(symbols))` threads the table.
pub struct DebugWith<'a> {
    value: Value,
    symbols: Option<&'a SymbolTable>,
}

impl fmt::Display for DebugWith<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_value(&self.value, self.symbols, true, f)
    }
}

impl Value {
    /// Render through `symbols`, resolving symbol names (`'name`); pass `None` to
    /// render `#<sym:id>`. The threaded-table alternative to a bare `Display`
    /// (docs/impl/region/ctx.md § "Symbols through the ctx").
    pub fn display_with<'a>(&self, symbols: Option<&'a SymbolTable>) -> DisplayWith<'a> {
        DisplayWith {
            value: *self,
            symbols,
        }
    }

    /// `Debug`-style render through `symbols` (quoted strings, resolved names).
    pub fn debug_with<'a>(&self, symbols: Option<&'a SymbolTable>) -> DebugWith<'a> {
        DebugWith {
            value: *self,
            symbols,
        }
    }
}

#[cfg(test)]
mod tests;
