//! Module-qualified symbol resolution (e.g., string:upcase -> string-upcase)

use super::Expander;

impl Expander {
    /// Resolve a qualified symbol like `string:upcase` to its flat primitive name.
    /// Returns None if the symbol is not qualified or the module is unknown.
    pub(super) fn resolve_qualified_symbol(&self, name: &str) -> Option<String> {
        // Check if it's a qualified name (contains ':' but doesn't start with ':')
        if name.starts_with(':') || !name.contains(':') {
            return None;
        }

        let parts: Vec<&str> = name.splitn(2, ':').collect();
        if parts.len() != 2 {
            return None;
        }

        let module = parts[0];
        let func = parts[1];

        // Module-specific resolution rules
        match module {
            "string" => {
                // string:upcase -> string-upcase, string:length -> string-length, etc.
                Some(format!("string-{}", func))
            }
            "math" => {
                // math:abs -> abs, math:floor -> floor, etc.
                // Math functions are registered with their short names
                Some(func.to_string())
            }
            "list" => {
                // list:length -> length, list:append -> append, etc.
                // List functions are registered with their short names
                Some(func.to_string())
            }
            "json" => {
                // json:parse -> json-parse, json:serialize -> json-serialize
                Some(format!("json-{}", func))
            }
            _ => None, // Unknown module
        }
    }
}
