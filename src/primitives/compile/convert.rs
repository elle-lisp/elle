use super::*;
use crate::primitives::ctx::NativeCtx;

pub(crate) fn signal_to_value(sig: &Signal, ctx: &mut NativeCtx) -> Value {
    let mut fields = BTreeMap::new();

    // :bits as keyword set. A learning site for the same reason `(signals)` is
    // one: the registry carries names a program declared at run time, and this
    // instance may never have met the spelling (docs/impl/symbol.md § "The
    // display memo").
    let names = with_registry(|reg| {
        reg.entries()
            .iter()
            .filter(|e| sig.bits.has_bit(e.bit_position))
            .map(|e| e.name.clone())
            .collect::<Vec<_>>()
    });
    let bit_set: BTreeSet<Value> = names.iter().map(|n| ctx.keyword(n)).collect();
    let bits_val = ctx.set(bit_set);
    fields.insert(kw("bits"), bits_val);

    // :propagates as integer set
    let mut prop_set = BTreeSet::new();
    for i in 0..32u32 {
        if sig.propagates & (1 << i) != 0 {
            prop_set.insert(Value::int(i as i64));
        }
    }
    let prop_val = ctx.set(prop_set);
    fields.insert(kw("propagates"), prop_val);

    // Derived convenience booleans
    let silent = sig.bits.is_empty() && sig.propagates == 0;
    let yields = sig.may_suspend();
    let io = sig.bits.intersects(crate::signals::SIG_IO);
    fields.insert(kw("silent"), Value::bool(silent));
    fields.insert(kw("yields"), Value::bool(yields));
    fields.insert(kw("io"), Value::bool(io));
    fields.insert(kw("jit-eligible"), Value::bool(!yields));

    ctx.struct_from(fields)
}
pub(crate) fn diagnostic_to_value(d: &Diagnostic, ctx: &mut NativeCtx) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        kw("severity"),
        Value::keyword(match d.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }),
    );
    let code = ctx.string(&*d.code);
    fields.insert(kw("code"), code);
    let rule = ctx.string(&*d.rule);
    fields.insert(kw("rule"), rule);
    let message = ctx.string(&*d.message);
    fields.insert(kw("message"), message);
    if let Some(func) = &d.function {
        let func_val = ctx.string(&**func);
        fields.insert(kw("function"), func_val);
    }
    if let Some(loc) = &d.location {
        fields.insert(kw("line"), Value::int(loc.line as i64));
        fields.insert(kw("col"), Value::int(loc.col as i64));
    }
    let suggestions: Vec<Value> = d.suggestions.iter().map(|s| ctx.string(&**s)).collect();
    let suggestions_val = ctx.array(suggestions);
    fields.insert(kw("suggestions"), suggestions_val);
    ctx.struct_from(fields)
}
pub(crate) fn symbol_def_to_value(def: &SymbolDef, ctx: &mut NativeCtx) -> Value {
    let mut fields = BTreeMap::new();
    let name = ctx.string(&*def.name);
    fields.insert(kw("name"), name);
    fields.insert(
        kw("kind"),
        Value::keyword(match def.kind {
            SymbolKind::Function => "function",
            SymbolKind::Variable => "variable",
            SymbolKind::Builtin => "builtin",
            SymbolKind::Macro => "macro",
            SymbolKind::Module => "module",
        }),
    );
    if let Some(loc) = &def.location {
        fields.insert(kw("line"), Value::int(loc.line as i64));
        fields.insert(kw("col"), Value::int(loc.col as i64));
    }
    if let Some(arity) = def.arity {
        fields.insert(kw("arity"), Value::int(arity as i64));
    }
    if let Some(doc) = &def.documentation {
        let doc_val = ctx.string(&**doc);
        fields.insert(kw("doc"), doc_val);
    }
    ctx.struct_from(fields)
}
pub(crate) fn call_edge_to_value(edge: &CallEdge, ctx: &mut NativeCtx) -> Value {
    let mut fields = BTreeMap::new();
    let name = ctx.string(&*edge.callee);
    fields.insert(kw("name"), name);
    fields.insert(kw("line"), Value::int(edge.line as i64));
    fields.insert(kw("col"), Value::int(edge.col as i64));
    fields.insert(kw("tail"), Value::bool(edge.is_tail));
    ctx.struct_from(fields)
}
