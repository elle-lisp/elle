// audited: 2026-09-21
//! `compile/apply-rules` — the rewrite edit engine driven by rules as data.
//! docs/analysis/portrait.md

use std::collections::BTreeMap;

use crate::primitives::compile::kw;
use crate::rewrite::library::{apply_library_rules, LibraryRules};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::sorted_struct_get;
use crate::value::{TableKey, Value};

type Fields<'a> = &'a [(TableKey, Value)];

fn field_string(fields: Fields<'_>, key: &str) -> Option<String> {
    sorted_struct_get(fields, &TableKey::keyword(key))
        .and_then(|v| v.with_string(|s| s.to_string()))
}

fn field_int(fields: Fields<'_>, key: &str) -> Option<i64> {
    sorted_struct_get(fields, &TableKey::keyword(key)).and_then(|v| v.as_int())
}

/// `(compile/apply-rules source rules)` → `{:source :count :reports}`.
///
/// docs/analysis/portrait.md § "Rule-driven rewriting" is the
/// specification; tests/elle/compile-apply-rules.lisp pins it.
pub(crate) fn prim_compile_apply_rules(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let Some(source) = args[0].with_string(|s| s.to_string()) else {
        return (
            SIG_ERROR,
            ctx.error("type-error", "compile/apply-rules: SOURCE must be a string"),
        );
    };
    let Some(items) = args[1].as_array() else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "compile/apply-rules: RULES must be an array of rule structs",
            ),
        );
    };

    let mut rules = LibraryRules::default();
    for item in items {
        let Some(fields) = item.as_struct() else {
            return (
                SIG_ERROR,
                ctx.error("type-error", "compile/apply-rules: each rule is a struct"),
            );
        };
        let kind = sorted_struct_get(fields, &TableKey::keyword("kind"))
            .and_then(|v| ctx.keyword_spelling(*v))
            .unwrap_or_default();
        let complete = match kind.as_str() {
            "rename" => match (field_string(fields, "from"), field_string(fields, "to")) {
                (Some(from), Some(to)) => {
                    rules.renames.insert(from, to);
                    true
                }
                _ => false,
            },
            "replace" => match (
                field_string(fields, "name"),
                field_int(fields, "arity"),
                field_string(fields, "template"),
            ) {
                (Some(name), Some(arity), Some(template)) if arity >= 0 => {
                    rules.replaces.push((name, arity as usize, template));
                    true
                }
                _ => false,
            },
            "report" => match (
                field_string(fields, "name"),
                field_string(fields, "message"),
            ) {
                (Some(name), Some(message)) => {
                    rules.reports.push((name, message));
                    true
                }
                _ => false,
            },
            other => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "compile/apply-rules: unknown rule kind :{}; \
                             the vocabulary is :rename, :replace, :report",
                            other
                        ),
                    ),
                )
            }
        };
        if !complete {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("compile/apply-rules: malformed :{} rule", kind),
                ),
            );
        }
    }

    match apply_library_rules(&source, "<apply-rules>", &rules) {
        Ok(applied) => {
            let reports: Vec<Value> = applied
                .reports
                .iter()
                .map(|o| {
                    let mut fields = BTreeMap::new();
                    fields.insert(kw("name"), ctx.string(&o.name));
                    fields.insert(kw("message"), ctx.string(&o.message));
                    fields.insert(kw("line"), Value::int(o.line as i64));
                    ctx.struct_from(fields)
                })
                .collect();
            let reports_val = ctx.array(reports);
            let mut out = BTreeMap::new();
            out.insert(kw("source"), ctx.string(&applied.source));
            out.insert(kw("count"), Value::int(applied.count as i64));
            out.insert(kw("reports"), reports_val);
            (SIG_OK, ctx.struct_from(out))
        }
        Err(e) => (
            SIG_ERROR,
            ctx.error("rewrite-error", format!("compile/apply-rules: {}", e)),
        ),
    }
}
