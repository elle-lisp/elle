// audited: 2026-09-21
// The fn/signature primitive: one struct describing how a function is called.
// docs/functions.md
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// The parameter counts an [`Arity`] declares: required, optional, and
/// whether a rest collector follows. `num_params` counts every slot
/// including the collector's, so an `AtLeast` shape recovers its optional
/// count from it; a caller with no slot count passes 0 and gets none.
fn arity_shape(arity: Arity, num_params: usize) -> (usize, usize, bool) {
    match arity {
        Arity::Exact(n) => (n, 0, false),
        Arity::Range(min, max) => (min, max - min, false),
        Arity::AtLeast(min) => (min, num_params.saturating_sub(min + 1), true),
    }
}

/// (fn/signature f) — the declared shape of a function.
///
/// docs/functions.md § "fn/signature" is the specification; the struct's
/// field set is pinned by tests/elle/fn-signature.lisp.
pub(crate) fn prim_fn_signature(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    use crate::value::closure::VarargTag;
    use crate::value::TableKey;
    use std::collections::BTreeMap;
    let kw = TableKey::keyword;

    if let Some(closure) = args[0].as_closure() {
        let t = &closure.template;
        let mut fields = BTreeMap::new();
        if let Some(name) = t.name() {
            fields.insert(kw("name"), ctx.string(name));
        }
        let (required, optional, has_rest) = arity_shape(t.arity(), t.num_params());
        fields.insert(kw("required"), Value::int(required as i64));
        fields.insert(kw("optional"), Value::int(optional as i64));
        let rest = if !has_rest {
            "none"
        } else {
            match t.vararg_tag() {
                VarargTag::List => "list",
                VarargTag::Struct => "keys",
                VarargTag::StrictStruct => "named",
            }
        };
        fields.insert(kw("rest"), Value::keyword(rest));
        let mut keys: Vec<&str> = t.strict_keys().iter().collect();
        keys.sort_unstable();
        // The spellings arrive from user source at run time, so each is
        // minted through the instance memo, as `(keyword s)` is.
        let keys: Vec<Value> = keys.into_iter().map(|k| ctx.keyword(k)).collect();
        let keys_val = ctx.array(keys);
        fields.insert(kw("named-keys"), keys_val);
        let signal = t.signal();
        let signals = crate::primitives::compile::signal_to_value(&signal, ctx);
        fields.insert(kw("signals"), signals);
        if let Some(doc) = t.doc() {
            fields.insert(kw("doc"), ctx.string(doc));
        }
        if let Some(span) = t.origin() {
            if let Some(file) = span.file() {
                let mut origin = BTreeMap::new();
                origin.insert(kw("file"), ctx.string(file));
                origin.insert(kw("line"), Value::int(span.line as i64));
                origin.insert(kw("col"), Value::int(span.col as i64));
                let origin_val = ctx.struct_from(origin);
                fields.insert(kw("origin"), origin_val);
            }
        }
        return (SIG_OK, ctx.struct_from(fields));
    }
    if let Some(def) = args[0].as_native_def() {
        let mut fields = BTreeMap::new();
        fields.insert(kw("name"), ctx.string(def.name));
        let (required, optional, has_rest) = arity_shape(def.arity, 0);
        fields.insert(kw("required"), Value::int(required as i64));
        fields.insert(kw("optional"), Value::int(optional as i64));
        let rest = if has_rest { "list" } else { "none" };
        fields.insert(kw("rest"), Value::keyword(rest));
        let keys_val = ctx.array(Vec::new());
        fields.insert(kw("named-keys"), keys_val);
        let signals = crate::primitives::compile::signal_to_value(&def.signal, ctx);
        fields.insert(kw("signals"), signals);
        if !def.doc.is_empty() {
            fields.insert(kw("doc"), ctx.string(def.doc));
        }
        return (SIG_OK, ctx.struct_from(fields));
    }
    (
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "fn/signature: expected function, got {}",
                args[0].type_name()
            ),
        ),
    )
}

primitive! {
    "fn/signature" => prim_fn_signature {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the declared shape of a function: {:name :required :optional :rest \
              :named-keys :signals :doc :origin}. Errors on a non-function.",
        params: &["f"],
        category: "fn",
        example: "(fn/signature (fn [a &opt b] a))",
        effect: RegionEffect::Fresh,
    }
}
