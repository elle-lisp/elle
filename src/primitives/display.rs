use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// Generate a flat single-line representation of a value
fn flat_repr(val: Value, depth: usize) -> String {
    // Depth limit to prevent infinite recursion on circular structures
    if depth > 10 {
        return "...".to_string();
    }

    // Simple immediate values
    if val.is_nil() {
        return "nil".to_string();
    }

    if val.is_empty_list() {
        return "()".to_string();
    }

    if let Some(b) = val.as_bool() {
        return b.to_string();
    }

    if let Some(n) = val.as_int() {
        return n.to_string();
    }

    if let Some(n) = val.as_float() {
        return n.to_string();
    }

    if let Some(r) = val.with_string(|s| {
        // Truncate long strings at 60 chars with ...
        if s.len() > 60 {
            format!("\"{}...\"", &s[..60])
        } else {
            format!("\"{}\"", s)
        }
    }) {
        return r;
    }

    if let Some(_id) = val.as_symbol() {
        return val.to_string();
    }

    if val.as_keyword_name().is_some() {
        return val.to_string();
    }

    // Lists
    if let Some(_cons) = val.as_cons() {
        let mut parts = Vec::new();
        let mut current = val;
        while let Some(cons) = current.as_cons() {
            parts.push(flat_repr(cons.first, depth + 1));
            current = cons.rest;
        }
        if !current.is_empty_list() && !current.is_nil() {
            parts.push(format!(". {}", flat_repr(current, depth + 1)));
        }
        return format!("({})", parts.join(" "));
    }

    // Arrays
    if let Some(vec_ref) = val.as_array_mut() {
        let vec = vec_ref.borrow();
        let parts: Vec<String> = vec.iter().map(|v| flat_repr(*v, depth + 1)).collect();
        return format!("[{}]", parts.join(" "));
    }

    // Tables
    if let Some(table_ref) = val.as_struct_mut() {
        let table = table_ref.borrow();
        let mut parts = Vec::new();
        for (k, v) in table.iter() {
            parts.push(format!("{:?} {}", k, flat_repr(*v, depth + 1)));
        }
        return format!("{{{}}}", parts.join(" "));
    }

    // Structs
    if let Some(struct_map) = val.as_struct() {
        let mut parts = Vec::new();
        for (k, v) in struct_map.iter() {
            parts.push(format!("{:?} {}", k, flat_repr(*v, depth + 1)));
        }
        return format!("#{{{}}}", parts.join(" "));
    }

    // Closures
    if val.is_closure() {
        return "<closure>".to_string();
    }

    // Fibers
    if val.as_fiber().is_some() {
        return "<fiber>".to_string();
    }

    // Native functions
    if val.as_native_fn().is_some() {
        return "<native-fn>".to_string();
    }

    // Default fallback
    val.to_string()
}

/// Pretty-print a value with width-aware line breaking
fn pretty_print_impl(val: Value, indent: usize, remaining_width: usize, depth: usize) -> String {
    const DEFAULT_WIDTH: usize = 80;

    // Depth limit
    if depth > 10 {
        return "...".to_string();
    }

    // Get flat representation
    let flat = flat_repr(val, depth);

    // If it fits on one line, use it
    if flat.len() <= remaining_width {
        return flat;
    }

    // Otherwise, break across lines based on value type
    let next_indent = indent + 2;
    let next_indent_str = " ".repeat(next_indent);

    // Simple values that can't be broken
    if val.is_nil()
        || val.is_empty_list()
        || val.as_bool().is_some()
        || val.as_int().is_some()
        || val.as_float().is_some()
        || val.as_symbol().is_some()
        || val.as_keyword_name().is_some()
        || val.is_closure()
        || val.as_fiber().is_some()
        || val.as_native_fn().is_some()
    {
        return flat;
    }

    // Strings: can't break, just return flat
    if val.is_string() {
        return flat;
    }

    // Lists: break with first element on same line as (
    if let Some(_cons) = val.as_cons() {
        let mut parts = Vec::new();
        let mut current = val;
        let mut first = true;

        while let Some(cons) = current.as_cons() {
            let part = pretty_print_impl(
                cons.first,
                next_indent,
                DEFAULT_WIDTH - next_indent,
                depth + 1,
            );
            if first {
                parts.push(part);
                first = false;
            } else {
                parts.push(format!("{}{}", next_indent_str, part));
            }
            current = cons.rest;
        }

        if !current.is_empty_list() && !current.is_nil() {
            let tail =
                pretty_print_impl(current, next_indent, DEFAULT_WIDTH - next_indent, depth + 1);
            parts.push(format!("{}. {}", next_indent_str, tail));
        }

        return format!("({})", parts.join("\n"));
    }

    // Arrays: break with elements indented
    if let Some(vec_ref) = val.as_array_mut() {
        let vec = vec_ref.borrow();
        if vec.is_empty() {
            return "[]".to_string();
        }
        let mut parts = Vec::new();
        for v in vec.iter() {
            let part = pretty_print_impl(*v, next_indent, DEFAULT_WIDTH - next_indent, depth + 1);
            parts.push(format!("{}{}", next_indent_str, part));
        }
        return format!("[\n{}]", parts.join("\n"));
    }

    // Tables: break with key-value pairs indented
    if let Some(table_ref) = val.as_struct_mut() {
        let table = table_ref.borrow();
        if table.is_empty() {
            return "{}".to_string();
        }
        let mut parts = Vec::new();
        for (k, v) in table.iter() {
            let v_str = pretty_print_impl(*v, next_indent, DEFAULT_WIDTH - next_indent, depth + 1);
            parts.push(format!("{}{:?} {}", next_indent_str, k, v_str));
        }
        return format!("{{\n{}}}", parts.join("\n"));
    }

    // Structs: break with key-value pairs indented
    if let Some(struct_map) = val.as_struct() {
        if struct_map.is_empty() {
            return "#{}".to_string();
        }
        let mut parts = Vec::new();
        for (k, v) in struct_map.iter() {
            let v_str = pretty_print_impl(*v, next_indent, DEFAULT_WIDTH - next_indent, depth + 1);
            parts.push(format!("{}{:?} {}", next_indent_str, k, v_str));
        }
        return format!("#{{\\n{}}}", parts.join("\n"));
    }

    // Fallback
    flat
}

/// (pp value) — Pretty-print a value with indentation, returns the value
pub(crate) fn prim_pp(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_OK, Value::NIL);
    }
    let val = args[0];

    const DEFAULT_WIDTH: usize = 80;
    let output = pretty_print_impl(val, 0, DEFAULT_WIDTH, 0);
    println!("{}", output);

    (SIG_OK, val)
}

/// (describe value) — Return a string describing a value's type and content
pub(crate) fn prim_describe(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (SIG_OK, Value::string("<error>"));
    }

    let val = args[0];

    if val.is_nil() {
        return (SIG_OK, Value::string("<nil>"));
    }

    if val.is_empty_list() {
        return (SIG_OK, Value::string("<list (0 elements)>"));
    }

    if let Some(b) = val.as_bool() {
        return (SIG_OK, Value::string(format!("<boolean {}>", b)));
    }

    if let Some(n) = val.as_int() {
        return (SIG_OK, Value::string(format!("<integer {}>", n)));
    }

    if let Some(n) = val.as_float() {
        return (SIG_OK, Value::string(format!("<float {}>", n)));
    }

    if let Some(r) = val.with_string(|s| {
        let display = if s.len() > 20 {
            format!("\"{}...\"", &s[..20])
        } else {
            format!("\"{}\"", s)
        };
        Value::string(format!("<string {} ({} chars)>", display, s.len()))
    }) {
        return (SIG_OK, r);
    }

    if let Some(buf_ref) = val.as_string_mut() {
        let buf = buf_ref.borrow();
        let display = if buf.len() > 20 {
            let s = String::from_utf8_lossy(&buf[..20]);
            format!("\"{}...\"", s)
        } else {
            let s = String::from_utf8_lossy(&buf);
            format!("\"{}\"", s)
        };
        return (
            SIG_OK,
            Value::string(format!("<@string {} ({} bytes)>", display, buf.len())),
        );
    }

    if let Some(_id) = val.as_symbol() {
        return (SIG_OK, Value::string(format!("<symbol {}>", val)));
    }

    if let Some(name) = val.as_keyword_name() {
        return (SIG_OK, Value::string(format!("<keyword :{}>", name)));
    }

    // Count list elements
    if let Some(_cons) = val.as_cons() {
        let mut count = 0;
        let mut current = val;
        while let Some(cons) = current.as_cons() {
            count += 1;
            current = cons.rest;
        }
        return (
            SIG_OK,
            Value::string(format!("<list ({} elements)>", count)),
        );
    }

    // Immutable array
    if let Some(elems) = val.as_array() {
        return (
            SIG_OK,
            Value::string(format!("<array ({} elements)>", elems.len())),
        );
    }

    // Mutable array
    if let Some(vec_ref) = val.as_array_mut() {
        let vec = vec_ref.borrow();
        return (
            SIG_OK,
            Value::string(format!("<@array ({} elements)>", vec.len())),
        );
    }

    // Immutable struct
    if let Some(struct_map) = val.as_struct() {
        return (
            SIG_OK,
            Value::string(format!("<struct ({} entries)>", struct_map.len())),
        );
    }

    // Mutable struct
    if let Some(table_ref) = val.as_struct_mut() {
        let table = table_ref.borrow();
        return (
            SIG_OK,
            Value::string(format!("<@struct ({} entries)>", table.len())),
        );
    }

    // Immutable set
    if let Some(set) = val.as_set() {
        return (
            SIG_OK,
            Value::string(format!("<set ({} elements)>", set.len())),
        );
    }

    // Mutable set
    if let Some(set_ref) = val.as_set_mut() {
        let set = set_ref.borrow();
        return (
            SIG_OK,
            Value::string(format!("<@set ({} elements)>", set.len())),
        );
    }

    // Immutable bytes
    if let Some(bytes) = val.as_bytes() {
        return (
            SIG_OK,
            Value::string(format!("<bytes ({} bytes)>", bytes.len())),
        );
    }

    // Mutable bytes
    if let Some(bytes_ref) = val.as_bytes_mut() {
        let bytes = bytes_ref.borrow();
        return (
            SIG_OK,
            Value::string(format!("<@bytes ({} bytes)>", bytes.len())),
        );
    }

    // Closure
    if let Some(closure) = val.as_closure() {
        let arity_str = match closure.template.arity {
            Arity::Exact(n) => format!("{}", n),
            Arity::AtLeast(n) => format!("{} or more", n),
            Arity::Range(min, max) => format!("{}-{}", min, max),
        };
        return (
            SIG_OK,
            Value::string(format!("<closure (arity {})>", arity_str)),
        );
    }

    // Box
    if val.as_lbox().is_some() {
        return (SIG_OK, Value::string("<cell>"));
    }

    // Fiber
    if val.as_fiber().is_some() {
        return (SIG_OK, Value::string("<fiber>"));
    }

    // Default
    (SIG_OK, Value::string("<unknown>"))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "pp",
        func: prim_pp,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Pretty-print a value with indentation. Returns the value.",
        params: &["value"],
        category: "io",
        example: "(pp (list 1 2 (list 3 4)))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "describe",
        func: prim_describe,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return a string describing a value's type and content.",
        params: &["value"],
        category: "io",
        example: "(describe (list 1 2 3)) #=> \"<list (3 elements)>\"",
        aliases: &[],
    },
];
