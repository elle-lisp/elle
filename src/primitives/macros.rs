//! Macro introspection primitives
//!
//! Provides runtime access to macro definitions for introspection and debugging.
//! Macros themselves expand at compile-time; these primitives allow querying
//! and manually expanding macros at runtime.

use crate::symbol::SymbolTable;
use crate::value::{Condition, Value};
use crate::value_old::SymbolId;
use std::cell::RefCell;

thread_local! {
    static SYMBOL_TABLE: RefCell<Option<*mut SymbolTable>> = const { RefCell::new(None) };
}

/// Set the symbol table context for macro primitives
///
/// # Safety
/// The pointer must remain valid for the duration of use.
/// This follows the same pattern as FFI's set_symbol_table.
pub fn set_macro_symbol_table(symbols: *mut SymbolTable) {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = Some(symbols);
    });
}

/// Clear the symbol table context
pub fn clear_macro_symbol_table() {
    SYMBOL_TABLE.with(|st| {
        *st.borrow_mut() = None;
    });
}

/// Access the symbol table safely
fn with_symbol_table<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut SymbolTable) -> Result<R, String>,
{
    SYMBOL_TABLE.with(|st| {
        let ptr = st.borrow();
        match *ptr {
            Some(p) => {
                // SAFETY: Caller ensures pointer validity via set_macro_symbol_table
                let symbols = unsafe { &mut *p };
                f(symbols)
            }
            None => Err("macro primitives: symbol table not available".into()),
        }
    })
}

/// Check if a value is a macro
///
/// (macro? symbol) => #t if symbol is defined as a macro, #f otherwise
///
/// # Examples
/// ```lisp
/// (defmacro my-macro (x) x)
/// (macro? my-macro)  ; => #t
/// (macro? +)         ; => #f
/// (macro? 42)        ; => #f
/// ```
pub fn prim_is_macro(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "macro?: expected 1 argument, got {}",
            args.len()
        )));
    }

    if let Some(sym_id) = args[0].as_symbol() {
        with_symbol_table(|symbols| Ok(Value::bool(symbols.is_macro(SymbolId(sym_id)))))
            .map_err(Condition::error)
    } else {
        // Non-symbols are never macros
        Ok(Value::bool(false))
    }
}

/// Expand a macro call and return the expanded form
///
/// (expand-macro '(macro-name arg1 arg2 ...)) => expanded form
///
/// The argument must be a quoted list where the first element is a macro name.
/// Returns the expanded form as data (not evaluated).
///
/// # Examples
/// ```lisp
/// (defmacro double (x) (* x 2))
/// (expand-macro '(double 5))  ; => (* 5 2)
///
/// (defmacro when (cond body) (list 'if cond body nil))
/// (expand-macro '(when #t (display "hi")))  ; => (if #t (display "hi") nil)
/// ```
pub fn prim_expand_macro(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "expand-macro: expected 1 argument, got {}",
            args.len()
        )));
    }

    let form = &args[0];

    // The form should be a list starting with a macro name
    let list = form.list_to_vec().map_err(|_| {
        Condition::type_error("expand-macro: argument must be a list (macro call form)".to_string())
    })?;

    if list.is_empty() {
        return Err(Condition::error("expand-macro: empty list".to_string()));
    }

    // First element should be a symbol (the macro name)
    let macro_sym = if let Some(id) = list[0].as_symbol() {
        id
    } else {
        return Err(Condition::type_error(
            "expand-macro: first element must be a symbol (macro name)".to_string(),
        ));
    };

    with_symbol_table(|symbols| {
        let sym_id = crate::value_old::SymbolId(macro_sym);
        // Check if it's actually a macro
        if !symbols.is_macro(sym_id) {
            let name = symbols.name(sym_id).unwrap_or("<unknown>");
            return Err(format!("expand-macro: '{}' is not a macro", name));
        }

        // Get the macro definition
        let macro_def = symbols
            .get_macro(sym_id)
            .ok_or_else(|| "expand-macro: macro definition not found".to_string())?;

        // Get the arguments (everything after the macro name)
        let macro_args = list[1..].to_vec();

        // Expand the macro using the existing expand_macro function
        use crate::compiler::macros::expand_macro;
        expand_macro(sym_id, &macro_def, &macro_args, symbols)
    })
    .map_err(Condition::error)
}
