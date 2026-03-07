//! Pre-scanning of expanded forms for forward declarations.

use crate::effects::Effect;
use crate::symbol::SymbolTable;
use crate::syntax::{Syntax, SyntaxKind};
use crate::value::types::Arity;
use crate::value::SymbolId;
use std::collections::{HashMap, HashSet};

/// Scan an expanded syntax form for `(var/def name (fn ...))` patterns.
/// Returns the SymbolId and syntactic arity if this is a binding-lambda form.
pub(super) fn scan_define_lambda(
    syntax: &Syntax,
    symbols: &mut SymbolTable,
) -> Option<(SymbolId, Option<Arity>)> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() >= 3 {
            if let Some(name) = items[0].as_symbol() {
                if name == "var" || name == "def" {
                    if let Some(def_name) = items[1].as_symbol() {
                        // Check if value is a lambda form
                        if let SyntaxKind::List(val_items) = &items[2].kind {
                            if let Some(first) = val_items.first() {
                                if let Some(kw) = first.as_symbol() {
                                    if kw == "fn" {
                                        let sym = symbols.intern(def_name);
                                        // Extract arity from the parameter list
                                        let arity = val_items
                                            .get(1)
                                            .and_then(|s| s.as_list())
                                            .map(|params| Arity::Exact(params.len()));
                                        return Some((sym, arity));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Scan an expanded syntax form for `(def name ...)` patterns.
/// Returns the SymbolId of the name if this is a def (immutable) form.
pub(super) fn scan_const_binding(syntax: &Syntax, symbols: &mut SymbolTable) -> Option<SymbolId> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() >= 3 {
            if let Some(name) = items[0].as_symbol() {
                if name == "def" {
                    if let Some(def_name) = items[1].as_symbol() {
                        return Some(symbols.intern(def_name));
                    }
                }
            }
        }
    }
    None
}

/// Pre-scan expanded forms for forward declarations.
///
/// Extracts:
/// - Lambda definitions: `(def/var name (fn ...))` -> seed global_effects
///   with `Effect::inert()`
/// - Arities: from lambda parameter lists
/// - Const bindings: `(def name ...)` -> immutable_globals
///
/// Returns `(global_effects, global_arities, immutable_globals)` to seed
/// the fixpoint loop.
pub(super) fn prescan_forms(
    forms: &[Syntax],
    symbols: &mut SymbolTable,
) -> (
    HashMap<SymbolId, Effect>,
    HashMap<SymbolId, Arity>,
    HashSet<SymbolId>,
) {
    let mut global_effects = HashMap::new();
    let mut global_arities = HashMap::new();
    let mut immutable_globals = HashSet::new();

    for form in forms {
        if let Some((sym, arity)) = scan_define_lambda(form, symbols) {
            global_effects.insert(sym, Effect::inert());
            if let Some(arity) = arity {
                global_arities.insert(sym, arity);
            }
        }
        if let Some(sym) = scan_const_binding(form, symbols) {
            immutable_globals.insert(sym);
        }
    }

    (global_effects, global_arities, immutable_globals)
}
