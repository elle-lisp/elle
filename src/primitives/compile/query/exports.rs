// audited: 2026-09-21
//! `compile/exports` — the module surface read statically off an analysis.
//! docs/analysis/portrait.md
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::hir::{Binding, Hir, HirKind, VarargKind};
use crate::primitives::compile::{get_handle, kw, signal_to_value, AnalysisHandle};
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::{TableKey, Value};

/// `(compile/exports analysis)` → `{:constructor <record|nil> :exports {..}}`,
/// or nil when the file's return expression is not an export struct.
///
/// docs/analysis/portrait.md § "Module exports" is the specification; the
/// record shape is pinned by tests/elle/compile-exports.lisp.
pub(in crate::primitives::compile) fn prim_compile_exports(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let handle = match get_handle(args, "compile/exports", ctx) {
        Ok(h) => h,
        Err(e) => return e,
    };
    // The instance's display memo, for parameter spellings. Read before the
    // walk so its borrow sits beside the handle's rather than inside it.
    let symbols_ptr = ctx.vm().symbols_ptr;
    let symbols = (!symbols_ptr.is_null()).then(|| unsafe { &*symbols_ptr });

    let mut walk = Walk {
        handle,
        symbols,
        env: HashMap::new(),
        seen: HashSet::new(),
        constructor: None,
    };
    let Some(struct_args) = walk.find_export_struct(&handle.hir) else {
        return (SIG_OK, Value::NIL);
    };

    // Alternating keyword/value arguments of the struct call, each value
    // resolved through the bindings the unwrap recorded on the way down.
    let mut export_records: Vec<(String, ExportRecord)> = Vec::new();
    let mut i = 0;
    while i + 1 < struct_args.len() {
        if let HirKind::Keyword(key) = &struct_args[i].expr.kind {
            let record = walk.record_for(&struct_args[i + 1].expr);
            export_records.push((key.clone(), record));
        }
        i += 2;
    }
    let constructor = walk.constructor;

    let mut exports = BTreeMap::new();
    for (key, record) in export_records {
        // The export key arrives from user source at run time, so it is
        // minted through the instance memo.
        let key_val = ctx.keyword(&key);
        let Some(key) = TableKey::from_value(&key_val) else {
            continue;
        };
        let record_val = record_to_value(&record, ctx);
        exports.insert(key, record_val);
    }
    let mut fields = BTreeMap::new();
    let exports_val = ctx.struct_from(exports);
    fields.insert(kw("exports"), exports_val);
    if let Some(c) = constructor {
        let c_val = record_to_value(&c, ctx);
        fields.insert(kw("constructor"), c_val);
    }
    (SIG_OK, ctx.struct_from(fields))
}

/// One export's static record: a function's declared shape, or a bare value.
enum ExportRecord {
    Fn {
        required: usize,
        optional: usize,
        rest: &'static str,
        named_keys: Vec<String>,
        params: Vec<String>,
        signals: crate::signals::Signal,
        doc: Option<String>,
        line: Option<u32>,
        col: Option<u32>,
    },
    Value,
}

fn record_to_value(record: &ExportRecord, ctx: &mut crate::primitives::ctx::NativeCtx) -> Value {
    let mut fields = BTreeMap::new();
    match record {
        ExportRecord::Value => {
            fields.insert(kw("kind"), Value::keyword("value"));
        }
        ExportRecord::Fn {
            required,
            optional,
            rest,
            named_keys,
            params,
            signals,
            doc,
            line,
            col,
        } => {
            fields.insert(kw("kind"), Value::keyword("fn"));
            fields.insert(kw("required"), Value::int(*required as i64));
            fields.insert(kw("optional"), Value::int(*optional as i64));
            fields.insert(kw("rest"), Value::keyword(rest));
            let keys: Vec<Value> = named_keys.iter().map(|k| ctx.keyword(k)).collect();
            let keys_val = ctx.array(keys);
            fields.insert(kw("named-keys"), keys_val);
            let names: Vec<Value> = params.iter().map(|p| ctx.string(p)).collect();
            let names_val = ctx.array(names);
            fields.insert(kw("params"), names_val);
            let signals_val = signal_to_value(signals, ctx);
            fields.insert(kw("signals"), signals_val);
            if let Some(doc) = doc {
                fields.insert(kw("doc"), ctx.string(doc));
            }
            if let Some(line) = line {
                fields.insert(kw("line"), Value::int(*line as i64));
            }
            if let Some(col) = col {
                fields.insert(kw("col"), Value::int(*col as i64));
            }
        }
    }
    ctx.struct_from(fields)
}

/// The unwrap from the file's root to its export struct. Bindings met on the
/// way down land in `env`, so an export value that is a `Var` resolves to the
/// initializer of the binding the export names — never to a nested helper
/// that shares its spelling.
struct Walk<'a> {
    handle: &'a AnalysisHandle,
    symbols: Option<&'a crate::symbol::SymbolTable>,
    env: HashMap<Binding, &'a Hir>,
    seen: HashSet<Binding>,
    constructor: Option<ExportRecord>,
}

impl<'a> Walk<'a> {
    /// Unwrap to the export struct's arguments. The first `Lambda` passed
    /// through is the module constructor.
    fn find_export_struct(&mut self, hir: &'a Hir) -> Option<&'a [crate::hir::CallArg]> {
        let mut expr = hir;
        loop {
            match &expr.kind {
                HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
                    for (b, init) in bindings {
                        self.env.insert(*b, init);
                    }
                    expr = body;
                }
                HirKind::Begin(exprs) => {
                    for e in exprs {
                        if let HirKind::Define { binding, value } = &e.kind {
                            self.env.insert(*binding, value);
                        }
                    }
                    expr = exprs.last()?;
                }
                HirKind::Lambda { body, .. } => {
                    if self.constructor.is_none() {
                        self.constructor = Some(self.lambda_record(expr));
                    }
                    expr = body;
                }
                HirKind::If { then_branch, .. } => {
                    // One representative branch; a module whose surface
                    // differs per branch has no single surface to record.
                    expr = then_branch;
                }
                HirKind::Var(binding) => {
                    if !self.seen.insert(*binding) {
                        return None;
                    }
                    expr = self.env.get(binding)?;
                }
                HirKind::Call { func, args, .. } => {
                    let HirKind::Var(head) = &func.kind else {
                        return None;
                    };
                    let head_name = self.handle.arena.get(*head).name;
                    if head_name != crate::value::SymbolId::of("struct") {
                        return None;
                    }
                    return Some(args);
                }
                _ => return None,
            }
        }
    }

    /// The record for one export value: resolve a `Var` through `env` to its
    /// initializer; a `Lambda` records its declared shape; anything else is
    /// a value export.
    fn record_for(&mut self, value: &'a Hir) -> ExportRecord {
        let mut expr = value;
        self.seen.clear();
        loop {
            match &expr.kind {
                HirKind::Lambda { .. } => return self.lambda_record(expr),
                HirKind::Var(binding) => {
                    if !self.seen.insert(*binding) {
                        return ExportRecord::Value;
                    }
                    match self.env.get(binding) {
                        Some(init) => expr = init,
                        None => return ExportRecord::Value,
                    }
                }
                _ => return ExportRecord::Value,
            }
        }
    }

    /// The declared shape of one `Lambda` node. `params` holds required,
    /// then optional, then the collector binding when one exists.
    fn lambda_record(&self, hir: &'a Hir) -> ExportRecord {
        let HirKind::Lambda {
            params,
            num_required,
            rest_param,
            vararg_kind,
            inferred_signals,
            doc,
            ..
        } = &hir.kind
        else {
            return ExportRecord::Value;
        };
        let fixed = params.len() - usize::from(rest_param.is_some());
        let (rest, named_keys) = match rest_param {
            None => ("none", Vec::new()),
            Some(_) => match vararg_kind {
                VarargKind::List => ("list", Vec::new()),
                VarargKind::Struct => ("keys", Vec::new()),
                VarargKind::StrictStruct(keys) => {
                    let mut keys = keys.clone();
                    keys.sort_unstable();
                    ("named", keys)
                }
            },
        };
        let names = params[..fixed]
            .iter()
            .map(|b| {
                let sym = self.handle.arena.get(*b).name;
                match self.param_name(sym) {
                    // A destructure pattern's synthetic holder has no
                    // user-written spelling.
                    Some(n) if !n.starts_with("__") => n,
                    _ => "_".to_string(),
                }
            })
            .collect();
        ExportRecord::Fn {
            required: *num_required,
            optional: fixed - num_required,
            rest,
            named_keys,
            params: names,
            signals: *inferred_signals,
            doc: doc.as_ref().map(|d| d.to_string()),
            line: (hir.span.line > 0).then_some(hir.span.line),
            col: (hir.span.line > 0).then_some(hir.span.col),
        }
    }

    /// A parameter's spelling, from the instance's display memo.
    fn param_name(&self, sym: crate::value::SymbolId) -> Option<String> {
        self.symbols?.name(sym).map(str::to_string)
    }
}
