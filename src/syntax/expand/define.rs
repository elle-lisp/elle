use super::*;

impl Expander {
    /// Handle (defmacro name (params...) body) or (var-macro name (params...) body)
    pub(super) fn handle_defmacro(
        &mut self,
        items: &[Syntax],
        span: &Span,
    ) -> Result<Syntax, String> {
        // Syntax: (defmacro name (params...) body)
        if items.len() != 4 {
            return Err(format!(
                "{}: defmacro requires exactly 3 arguments (name, parameters, body)",
                span
            ));
        }

        // Get macro name
        let name = items[1]
            .as_symbol()
            .ok_or_else(|| format!("{}: macro name must be a symbol", span))?
            .to_string();

        // Get parameter list
        let params_syntax = items[2].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[2].kind, SyntaxKind::ArrayMut(_)) {
                format!(
                    "{}: macro parameters must use (...) or [...], not @[...]",
                    items[2].span
                )
            } else {
                format!(
                    "{}: macro parameters must be a list (...) or [...], got {}",
                    items[2].span,
                    items[2].kind_label()
                )
            }
        })?;

        // Parse params: required* (&opt optional*)? (& rest)?
        let mut fixed_params = Vec::new();
        let mut optional_params = Vec::new();
        let mut rest_param = None;
        let mut in_optional = false;
        let mut i = 0;
        while i < params_syntax.len() {
            let p = params_syntax[i]
                .as_symbol()
                .ok_or_else(|| format!("{}: macro parameter must be a symbol", span))?;
            if p == "&opt" {
                in_optional = true;
                i += 1;
                continue;
            }
            if crate::syntax::is_rest_marker(p) {
                // Next symbol is the rest param
                if i + 1 >= params_syntax.len() {
                    return Err(format!("{}: expected parameter name after &", span));
                }
                if i + 2 < params_syntax.len() {
                    return Err(format!("{}: only one parameter allowed after &", span));
                }
                let rest_name = params_syntax[i + 1]
                    .as_symbol()
                    .ok_or_else(|| format!("{}: macro parameter must be a symbol", span))?;
                rest_param = Some(rest_name.to_string());
                break;
            }
            if in_optional {
                optional_params.push(p.to_string());
            } else {
                fixed_params.push(p.to_string());
            }
            i += 1;
        }

        // Get the body template
        let template = items[3].clone();

        // Create and register the macro
        let macro_def = MacroDef {
            name: name.clone(),
            params: fixed_params,
            optional_params,
            rest_param,
            template,
            cached_transformer: std::rc::Rc::new(RefCell::new(None)),
        };

        self.define_macro(macro_def);

        // Return nil - the macro definition itself doesn't produce code
        Ok(Syntax::new(SyntaxKind::Nil, span.clone()))
    }
}
