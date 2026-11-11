// audited: 2026-11-09
//! A qualified symbol, `a:b:c`, desugared to the `get` calls it stands for.
//!
//! docs/impl/hir.md
//! docs/impl/symbol.md

use super::*;

impl<'a> Analyzer<'a> {
    /// Desugar a qualified symbol like `a:b:c` to nested `get` calls:
    /// `(get (get a :b) :c)`.
    ///
    /// The first segment is resolved as a variable (local or global).
    /// Each subsequent segment becomes a keyword argument to `get`, and its
    /// spelling is recorded in the memo, because no keyword token spells it.
    /// All synthesized HIR nodes carry the original symbol's span.
    ///
    /// The `get` binding always resolves to the global primitive,
    /// matching the pattern used for array/struct literal
    /// desugaring (the SyntaxKind::Array/ArrayMut/Struct/StructMut arms in
    /// forms/expr.rs).
    pub(super) fn desugar_qualified_symbol(
        &mut self,
        name: &str,
        span: &Span,
        scopes: &[ScopeId],
    ) -> Result<Hir, String> {
        let segments: Vec<&str> = name.split(':').collect();
        // Reader guarantees: no empty segments, no leading colon (checked above),
        // at least 2 segments (contains ':' is true).

        // First segment: resolve as variable
        let first = segments[0];
        let found = self
            .lookup(first, scopes)
            .or_else(|| self.lookup(first, &[]));
        let mut result = match found {
            Some(binding) => {
                self.check_merge_read(binding, first, span)?;
                Hir::silent(HirKind::Var(binding), *span)
            }
            None => {
                let suggestions = self.suggest_similar(first);
                let error = span.undefined_var_suggest(first, suggestions);
                return Ok(self.accumulate_error(error, span));
            }
        };

        // Each subsequent segment: wrap in (get result :segment)
        // Constructs Call nodes directly (not via analyze_call) because
        // get is a primitive with known arity Range(2,3). It may raise, as a
        // direct `(get m :k)` may, so each step carries :error.
        let get_binding = self.resolve_primitive("get");
        self.add_inherent_bits(crate::value::SIG_ERROR);
        for segment in &segments[1..] {
            let get_func = Hir::silent(HirKind::Var(get_binding), *span);
            let key = Hir::silent(HirKind::Keyword(segment.to_string()), *span);
            self.symbols.keyword(segment);
            // Use projected signal if the binding has a projection for this field.
            let call_signal = if let HirKind::Var(binding) = &result.kind {
                if let Some(proj) = self.projection_env.get(binding) {
                    proj.get(*segment).copied().unwrap_or(result.signal)
                } else {
                    result.signal
                }
            } else {
                result.signal
            };
            let call_signal = call_signal.combine(Signal::errors());
            result = Hir::new(
                HirKind::Call {
                    func: Box::new(get_func),
                    args: vec![
                        CallArg {
                            expr: result,
                            spliced: false,
                        },
                        CallArg {
                            expr: key,
                            spliced: false,
                        },
                    ],
                    is_tail: false,
                },
                *span,
                call_signal,
            );
        }

        Ok(result)
    }
}
