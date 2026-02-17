use crate::symbol::{MacroDef, SymbolTable};
use crate::value::repr::Value;
use crate::value_old::SymbolId;
use std::rc::Rc;

/// Expand a macro by substituting its parameters with the provided arguments
pub fn expand_macro(
    _macro_name: SymbolId,
    macro_def: &Rc<MacroDef>,
    args: &[Value],
    symbols: &mut SymbolTable,
) -> Result<Value, String> {
    use crate::read_str;

    // Check argument count
    if args.len() != macro_def.params.len() {
        return Err(format!(
            "Macro expects {} arguments, got {}",
            macro_def.params.len(),
            args.len()
        ));
    }

    // Build a mapping of parameter names to their argument values
    // We need to get the names of the parameter symbols
    let mut param_mapping: Vec<(String, Value)> = Vec::new();
    for (param_id, arg_value) in macro_def.params.iter().zip(args.iter()) {
        if let Some(param_name) = symbols.name(*param_id) {
            param_mapping.push((param_name.to_string(), *arg_value));
        }
    }

    // Parse the macro body from its source code
    // This will create NEW symbol IDs for any symbols in the body
    let body_value = read_str(&macro_def.body, symbols)
        .map_err(|e| format!("Failed to parse macro body: {}", e))?;

    // Now substitute by name, so we can handle symbol ID mismatches
    Ok(substitute_params_by_name(
        &body_value,
        &param_mapping,
        symbols,
    ))
}

/// Recursively substitute macro parameters with their arguments (by name)
pub fn substitute_params_by_name(
    value: &Value,
    param_mapping: &[(String, Value)],
    symbols: &SymbolTable,
) -> Value {
    if let Some(sym_id) = value.as_symbol() {
        // Get the name of this symbol
        if let Some(name) = symbols.name(SymbolId(sym_id)) {
            // Check if this is a parameter name
            if let Some((_param_name, arg_value)) =
                param_mapping.iter().find(|(pname, _)| pname == name)
            {
                return *arg_value;
            }
        }
        *value
    } else if value.is_cons() {
        // Recursively substitute in list/cons cells
        if let Ok(list_vec) = value.list_to_vec() {
            let new_items: Vec<Value> = list_vec
                .iter()
                .map(|item| substitute_params_by_name(item, param_mapping, symbols))
                .collect();
            crate::value::repr::list(new_items)
        } else {
            *value
        }
    } else {
        *value
    }
}
