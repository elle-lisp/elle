//! Elle syn plugin — Rust syntax parsing via the `syn` crate.

use elle::list;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use quote::ToTokens;
use std::collections::BTreeMap;
elle::elle_plugin_init!(PRIMITIVES, "syn/");

// ---------------------------------------------------------------------------
// Parsing primitives (stubs)
// ---------------------------------------------------------------------------

fn prim_syn_parse_file(args: &[Value]) -> (SignalBits, Value) {
    let src = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/parse-file: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match syn::parse_file(&src) {
        Ok(file) => (SIG_OK, Value::external("syn-file", file)),
        Err(e) => (
            SIG_ERROR,
            error_val("parse-error", format!("syn/parse-file: {}", e)),
        ),
    }
}

fn prim_syn_parse_expr(args: &[Value]) -> (SignalBits, Value) {
    let src = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/parse-expr: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match syn::parse_str::<syn::Expr>(&src) {
        Ok(expr) => (SIG_OK, Value::external("syn-expr", expr)),
        Err(e) => (
            SIG_ERROR,
            error_val("parse-error", format!("syn/parse-expr: {}", e)),
        ),
    }
}

fn prim_syn_parse_type(args: &[Value]) -> (SignalBits, Value) {
    let src = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/parse-type: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match syn::parse_str::<syn::Type>(&src) {
        Ok(ty) => (SIG_OK, Value::external("syn-type", ty)),
        Err(e) => (
            SIG_ERROR,
            error_val("parse-error", format!("syn/parse-type: {}", e)),
        ),
    }
}

fn prim_syn_parse_item(args: &[Value]) -> (SignalBits, Value) {
    let src = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/parse-item: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match syn::parse_str::<syn::Item>(&src) {
        Ok(item) => (SIG_OK, Value::external("syn-item", item)),
        Err(e) => (
            SIG_ERROR,
            error_val("parse-error", format!("syn/parse-item: {}", e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Navigation primitives (stubs)
// ---------------------------------------------------------------------------

fn prim_syn_items(args: &[Value]) -> (SignalBits, Value) {
    let file = match args[0].as_external::<syn::File>() {
        Some(f) => f,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("syn/items: expected syn-file, got {}", args[0].type_name()),
                ),
            );
        }
    };
    let items: Vec<Value> = file
        .items
        .iter()
        .map(|item| Value::external("syn-item", item.clone()))
        .collect();
    (SIG_OK, list(items))
}

fn prim_syn_item_kind(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/item-kind: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let kind = match item {
        syn::Item::Fn(_) => "fn",
        syn::Item::Struct(_) => "struct",
        syn::Item::Enum(_) => "enum",
        syn::Item::Trait(_) => "trait",
        syn::Item::Impl(_) => "impl",
        syn::Item::Use(_) => "use",
        syn::Item::Mod(_) => "mod",
        syn::Item::Const(_) => "const",
        syn::Item::Static(_) => "static",
        syn::Item::Type(_) => "type",
        syn::Item::Macro(_) => "macro",
        _ => "other",
    };
    (SIG_OK, Value::keyword(kind))
}

fn prim_syn_item_name(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/item-name: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let name: Option<String> = match item {
        syn::Item::Fn(f) => Some(f.sig.ident.to_string()),
        syn::Item::Struct(s) => Some(s.ident.to_string()),
        syn::Item::Enum(e) => Some(e.ident.to_string()),
        syn::Item::Trait(t) => Some(t.ident.to_string()),
        syn::Item::Mod(m) => Some(m.ident.to_string()),
        syn::Item::Const(c) => Some(c.ident.to_string()),
        syn::Item::Static(s) => Some(s.ident.to_string()),
        syn::Item::Type(t) => Some(t.ident.to_string()),
        syn::Item::Macro(m) => m.ident.as_ref().map(|i| i.to_string()),
        _ => None,
    };
    match name {
        Some(s) => (SIG_OK, Value::string(s.as_str())),
        None => (SIG_OK, Value::NIL),
    }
}

// ---------------------------------------------------------------------------
// Introspection helpers
// ---------------------------------------------------------------------------

/// Extract the start line number from a syn item's span.
/// Requires `proc-macro2` compiled with `span-locations` feature.
fn item_start_line(item: &syn::Item) -> Option<usize> {
    use syn::spanned::Spanned;
    let span = item.span();
    let start = span.start();
    if start.line == 0 {
        None // span-locations not available
    } else {
        Some(start.line)
    }
}

fn item_kind_str(item: &syn::Item) -> &'static str {
    match item {
        syn::Item::Fn(_) => "fn",
        syn::Item::Struct(_) => "struct",
        syn::Item::Enum(_) => "enum",
        syn::Item::Trait(_) => "trait",
        syn::Item::Impl(_) => "impl",
        syn::Item::Use(_) => "use",
        syn::Item::Mod(_) => "mod",
        syn::Item::Const(_) => "const",
        syn::Item::Static(_) => "static",
        syn::Item::Type(_) => "type",
        syn::Item::Macro(_) => "macro",
        _ => "other",
    }
}

fn fn_args_to_elle(sig: &syn::Signature) -> Value {
    let args: Vec<Value> = sig
        .inputs
        .iter()
        .map(|arg| {
            let mut fields = BTreeMap::new();
            match arg {
                syn::FnArg::Receiver(r) => {
                    fields.insert(TableKey::Keyword("name".into()), Value::string("self"));
                    let ty_str = if r.reference.is_some() {
                        if r.mutability.is_some() {
                            "&mut self".to_string()
                        } else {
                            "&self".to_string()
                        }
                    } else {
                        "self".to_string()
                    };
                    fields.insert(
                        TableKey::Keyword("type".into()),
                        Value::string(ty_str.as_str()),
                    );
                }
                syn::FnArg::Typed(pt) => {
                    let name_str = match pt.pat.as_ref() {
                        syn::Pat::Ident(pi) => pi.ident.to_string(),
                        _ => pt.pat.to_token_stream().to_string(),
                    };
                    let type_str = pt.ty.to_token_stream().to_string();
                    fields.insert(
                        TableKey::Keyword("name".into()),
                        Value::string(name_str.as_str()),
                    );
                    fields.insert(
                        TableKey::Keyword("type".into()),
                        Value::string(type_str.as_str()),
                    );
                }
            }
            Value::struct_from(fields)
        })
        .collect();
    list(args)
}

fn fields_to_elle(fields: &syn::Fields) -> (Value, Value) {
    match fields {
        syn::Fields::Named(named) => {
            let fs: Vec<Value> = named
                .named
                .iter()
                .map(|f| {
                    let mut m = BTreeMap::new();
                    let name_val = match &f.ident {
                        Some(i) => Value::string(i.to_string().as_str()),
                        None => Value::NIL,
                    };
                    let type_str = f.ty.to_token_stream().to_string();
                    m.insert(TableKey::Keyword("name".into()), name_val);
                    m.insert(
                        TableKey::Keyword("type".into()),
                        Value::string(type_str.as_str()),
                    );
                    Value::struct_from(m)
                })
                .collect();
            (Value::keyword("named"), list(fs))
        }
        syn::Fields::Unnamed(unnamed) => {
            let fs: Vec<Value> = unnamed
                .unnamed
                .iter()
                .map(|f| {
                    let mut m = BTreeMap::new();
                    let type_str = f.ty.to_token_stream().to_string();
                    m.insert(TableKey::Keyword("name".into()), Value::NIL);
                    m.insert(
                        TableKey::Keyword("type".into()),
                        Value::string(type_str.as_str()),
                    );
                    Value::struct_from(m)
                })
                .collect();
            (Value::keyword("tuple"), list(fs))
        }
        syn::Fields::Unit => (Value::keyword("unit"), list(vec![])),
    }
}

// ---------------------------------------------------------------------------
// Introspection primitives
// ---------------------------------------------------------------------------

fn prim_syn_fn_info(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-info: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let func = match item {
        syn::Item::Fn(f) => f,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-info: expected fn item, got :{}",
                        item_kind_str(item)
                    ),
                ),
            );
        }
    };
    let sig = &func.sig;
    let return_type_val = match &sig.output {
        syn::ReturnType::Default => Value::NIL,
        syn::ReturnType::Type(_, ty) => Value::string(ty.to_token_stream().to_string().as_str()),
    };
    let mut fields = BTreeMap::new();
    fields.insert(
        TableKey::Keyword("name".into()),
        Value::string(sig.ident.to_string().as_str()),
    );
    fields.insert(TableKey::Keyword("args".into()), fn_args_to_elle(sig));
    fields.insert(TableKey::Keyword("return-type".into()), return_type_val);
    fields.insert(
        TableKey::Keyword("async?".into()),
        Value::bool(sig.asyncness.is_some()),
    );
    fields.insert(
        TableKey::Keyword("unsafe?".into()),
        Value::bool(sig.unsafety.is_some()),
    );
    fields.insert(
        TableKey::Keyword("const?".into()),
        Value::bool(sig.constness.is_some()),
    );
    if let Some(line) = item_start_line(item) {
        fields.insert(TableKey::Keyword("line".into()), Value::int(line as i64));
    }
    (SIG_OK, Value::struct_from(fields))
}

fn prim_syn_fn_args(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-args: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let func = match item {
        syn::Item::Fn(f) => f,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-args: expected fn item, got :{}",
                        item_kind_str(item)
                    ),
                ),
            );
        }
    };
    (SIG_OK, fn_args_to_elle(&func.sig))
}

fn prim_syn_fn_return_type(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-return-type: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let func = match item {
        syn::Item::Fn(f) => f,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-return-type: expected fn item, got :{}",
                        item_kind_str(item)
                    ),
                ),
            );
        }
    };
    match &func.sig.output {
        syn::ReturnType::Default => (SIG_OK, Value::NIL),
        syn::ReturnType::Type(_, ty) => (
            SIG_OK,
            Value::string(ty.to_token_stream().to_string().as_str()),
        ),
    }
}

fn prim_syn_struct_fields(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/struct-fields: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let st = match item {
        syn::Item::Struct(s) => s,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/struct-fields: expected struct item, got :{}",
                        item_kind_str(item)
                    ),
                ),
            );
        }
    };
    let (kind_kw, fields_list) = fields_to_elle(&st.fields);
    let mut result = BTreeMap::new();
    result.insert(
        TableKey::Keyword("name".into()),
        Value::string(st.ident.to_string().as_str()),
    );
    result.insert(TableKey::Keyword("kind".into()), kind_kw);
    result.insert(TableKey::Keyword("fields".into()), fields_list);
    (SIG_OK, Value::struct_from(result))
}

fn prim_syn_enum_variants(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/enum-variants: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let en = match item {
        syn::Item::Enum(e) => e,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/enum-variants: expected enum item, got :{}",
                        item_kind_str(item)
                    ),
                ),
            );
        }
    };
    let variants: Vec<Value> = en
        .variants
        .iter()
        .map(|v| {
            let (kind_kw, fields_list) = fields_to_elle(&v.fields);
            let mut vm = BTreeMap::new();
            vm.insert(
                TableKey::Keyword("name".into()),
                Value::string(v.ident.to_string().as_str()),
            );
            vm.insert(TableKey::Keyword("kind".into()), kind_kw);
            vm.insert(TableKey::Keyword("fields".into()), fields_list);
            if let Some((_, disc_expr)) = &v.discriminant {
                let disc_str = disc_expr.to_token_stream().to_string();
                vm.insert(
                    TableKey::Keyword("discriminant".into()),
                    Value::string(disc_str.as_str()),
                );
            }
            Value::struct_from(vm)
        })
        .collect();
    let mut result = BTreeMap::new();
    result.insert(
        TableKey::Keyword("name".into()),
        Value::string(en.ident.to_string().as_str()),
    );
    result.insert(TableKey::Keyword("variants".into()), list(variants));
    (SIG_OK, Value::struct_from(result))
}

fn prim_syn_attributes(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/attributes: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let attrs: &[syn::Attribute] = match item {
        syn::Item::Fn(f) => &f.attrs,
        syn::Item::Struct(s) => &s.attrs,
        syn::Item::Enum(e) => &e.attrs,
        syn::Item::Trait(t) => &t.attrs,
        syn::Item::Impl(i) => &i.attrs,
        syn::Item::Use(u) => &u.attrs,
        syn::Item::Mod(m) => &m.attrs,
        syn::Item::Const(c) => &c.attrs,
        syn::Item::Static(s) => &s.attrs,
        syn::Item::Type(t) => &t.attrs,
        syn::Item::Macro(m) => &m.attrs,
        _ => return (SIG_OK, list(vec![])),
    };
    let attr_strs: Vec<Value> = attrs
        .iter()
        .map(|a| Value::string(a.to_token_stream().to_string().as_str()))
        .collect();
    (SIG_OK, list(attr_strs))
}

fn prim_syn_visibility(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/visibility: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let vis: Option<&syn::Visibility> = match item {
        syn::Item::Fn(f) => Some(&f.vis),
        syn::Item::Struct(s) => Some(&s.vis),
        syn::Item::Enum(e) => Some(&e.vis),
        syn::Item::Trait(t) => Some(&t.vis),
        syn::Item::Const(c) => Some(&c.vis),
        syn::Item::Static(s) => Some(&s.vis),
        syn::Item::Type(t) => Some(&t.vis),
        syn::Item::Mod(m) => Some(&m.vis),
        _ => None,
    };
    let kw = match vis {
        None => "private",
        Some(syn::Visibility::Public(_)) => "public",
        Some(syn::Visibility::Restricted(r)) => {
            let path_str = r.path.to_token_stream().to_string();
            if path_str == "crate" {
                "pub-crate"
            } else if path_str == "super" {
                "pub-super"
            } else {
                "pub-in"
            }
        }
        Some(syn::Visibility::Inherited) => "private",
    };
    (SIG_OK, Value::keyword(kw))
}

// ---------------------------------------------------------------------------
// Call site extraction
// ---------------------------------------------------------------------------

/// Recursively walk an expression tree collecting function call names.
fn collect_calls(expr: &syn::Expr, calls: &mut Vec<String>) {
    match expr {
        // foo(args) or path::to::foo(args)
        syn::Expr::Call(call) => {
            if let syn::Expr::Path(ep) = &*call.func {
                let name = path_to_string(&ep.path);
                if !name.is_empty() {
                    calls.push(name);
                }
            }
            // Recurse into the function expression and arguments.
            collect_calls(&call.func, calls);
            for arg in &call.args {
                collect_calls(arg, calls);
            }
        }
        // x.method(args)
        syn::Expr::MethodCall(mc) => {
            calls.push(mc.method.to_string());
            collect_calls(&mc.receiver, calls);
            for arg in &mc.args {
                collect_calls(arg, calls);
            }
        }
        // Recurse into all sub-expressions.
        syn::Expr::Block(b) => {
            for stmt in &b.block.stmts {
                collect_calls_stmt(stmt, calls);
            }
        }
        syn::Expr::If(ei) => {
            collect_calls(&ei.cond, calls);
            for stmt in &ei.then_branch.stmts {
                collect_calls_stmt(stmt, calls);
            }
            if let Some((_, else_branch)) = &ei.else_branch {
                collect_calls(else_branch, calls);
            }
        }
        syn::Expr::Match(m) => {
            collect_calls(&m.expr, calls);
            for arm in &m.arms {
                collect_calls(&arm.body, calls);
                if let Some(guard) = &arm.guard {
                    collect_calls(&guard.1, calls);
                }
            }
        }
        syn::Expr::Let(l) => {
            collect_calls(&l.expr, calls);
        }
        syn::Expr::Binary(b) => {
            collect_calls(&b.left, calls);
            collect_calls(&b.right, calls);
        }
        syn::Expr::Unary(u) => {
            collect_calls(&u.expr, calls);
        }
        syn::Expr::Reference(r) => {
            collect_calls(&r.expr, calls);
        }
        syn::Expr::Return(r) => {
            if let Some(expr) = &r.expr {
                collect_calls(expr, calls);
            }
        }
        syn::Expr::Paren(p) => {
            collect_calls(&p.expr, calls);
        }
        syn::Expr::Field(f) => {
            collect_calls(&f.base, calls);
        }
        syn::Expr::Index(i) => {
            collect_calls(&i.expr, calls);
            collect_calls(&i.index, calls);
        }
        syn::Expr::Tuple(t) => {
            for elem in &t.elems {
                collect_calls(elem, calls);
            }
        }
        syn::Expr::Array(a) => {
            for elem in &a.elems {
                collect_calls(elem, calls);
            }
        }
        syn::Expr::Struct(s) => {
            for field in &s.fields {
                collect_calls(&field.expr, calls);
            }
        }
        syn::Expr::Closure(c) => {
            collect_calls(&c.body, calls);
        }
        syn::Expr::Assign(a) => {
            collect_calls(&a.left, calls);
            collect_calls(&a.right, calls);
        }
        syn::Expr::Range(r) => {
            if let Some(start) = &r.start {
                collect_calls(start, calls);
            }
            if let Some(end) = &r.end {
                collect_calls(end, calls);
            }
        }
        syn::Expr::Try(t) => {
            collect_calls(&t.expr, calls);
        }
        syn::Expr::Await(a) => {
            collect_calls(&a.base, calls);
        }
        syn::Expr::Cast(c) => {
            collect_calls(&c.expr, calls);
        }
        syn::Expr::ForLoop(f) => {
            collect_calls(&f.expr, calls);
            for stmt in &f.body.stmts {
                collect_calls_stmt(stmt, calls);
            }
        }
        syn::Expr::While(w) => {
            collect_calls(&w.cond, calls);
            for stmt in &w.body.stmts {
                collect_calls_stmt(stmt, calls);
            }
        }
        syn::Expr::Loop(l) => {
            for stmt in &l.body.stmts {
                collect_calls_stmt(stmt, calls);
            }
        }
        syn::Expr::Unsafe(u) => {
            for stmt in &u.block.stmts {
                collect_calls_stmt(stmt, calls);
            }
        }
        _ => {}
    }
}

fn collect_calls_stmt(stmt: &syn::Stmt, calls: &mut Vec<String>) {
    match stmt {
        syn::Stmt::Expr(expr, _) => collect_calls(expr, calls),
        syn::Stmt::Local(local) => {
            if let Some(init) = &local.init {
                collect_calls(&init.expr, calls);
                if let Some((_, diverge)) = &init.diverge {
                    collect_calls(diverge, calls);
                }
            }
        }
        syn::Stmt::Item(_) => {}
        syn::Stmt::Macro(m) => {
            // Record macro invocations by name.
            let name = path_to_string(&m.mac.path);
            if !name.is_empty() {
                calls.push(name);
            }
        }
    }
}

/// Convert a syn::Path to a string like "crate::module::func".
fn path_to_string(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

/// (syn/fn-calls item) → ["func_a" "module::func_b" "method_name" ...]
fn prim_syn_fn_calls(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/fn-calls: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let block = match item {
        syn::Item::Fn(f) => &f.block,
        _ => {
            return (
                SIG_ERROR,
                error_val("type-error", "syn/fn-calls: item must be a function"),
            );
        }
    };
    let mut calls = Vec::new();
    for stmt in &block.stmts {
        collect_calls_stmt(stmt, &mut calls);
    }
    // Deduplicate while preserving order.
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<Value> = calls
        .into_iter()
        .filter(|c| seen.insert(c.clone()))
        .map(|c| Value::string(c.as_str()))
        .collect();
    (SIG_OK, Value::array(unique))
}

/// (syn/static-strings item) → ["value1" "value2" ...]
/// Extract all string literal values from a static item (e.g. PrimitiveDef arrays).
fn prim_syn_static_strings(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/static-strings: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let expr = match item {
        syn::Item::Static(s) => &*s.expr,
        syn::Item::Const(c) => &*c.expr,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "syn/static-strings: item must be static or const",
                ),
            );
        }
    };
    let mut strings = Vec::new();
    collect_string_lits(expr, &mut strings);
    let values: Vec<Value> = strings.iter().map(|s| Value::string(s.as_str())).collect();
    (SIG_OK, Value::array(values))
}

fn collect_string_lits(expr: &syn::Expr, strings: &mut Vec<String>) {
    match expr {
        syn::Expr::Lit(lit) => {
            if let syn::Lit::Str(s) = &lit.lit {
                strings.push(s.value());
            }
        }
        syn::Expr::Array(a) => {
            for elem in &a.elems {
                collect_string_lits(elem, strings);
            }
        }
        syn::Expr::Reference(r) => {
            collect_string_lits(&r.expr, strings);
        }
        syn::Expr::Struct(s) => {
            for field in &s.fields {
                collect_string_lits(&field.expr, strings);
            }
        }
        syn::Expr::Block(b) => {
            for stmt in &b.block.stmts {
                if let syn::Stmt::Expr(e, _) = stmt {
                    collect_string_lits(e, strings);
                }
            }
        }
        _ => {}
    }
}

/// (syn/primitive-defs item) → [{:name "elle/name" :func "rust_fn"} ...]
/// Extract name→func pairs from a PRIMITIVES const (PrimitiveDef array).
fn prim_syn_primitive_defs(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/primitive-defs: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let expr = match item {
        syn::Item::Const(c) => &*c.expr,
        syn::Item::Static(s) => &*s.expr,
        _ => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "syn/primitive-defs: item must be static or const",
                ),
            );
        }
    };
    // Walk the expression looking for struct literals with `name` and `func` fields.
    let mut results = Vec::new();
    collect_primitive_defs(expr, &mut results);
    (SIG_OK, Value::array(results))
}

fn collect_primitive_defs(expr: &syn::Expr, results: &mut Vec<Value>) {
    match expr {
        syn::Expr::Struct(s) => {
            let mut name_val: Option<String> = None;
            let mut func_val: Option<String> = None;
            for field in &s.fields {
                if let syn::Member::Named(ident) = &field.member {
                    let field_name = ident.to_string();
                    if field_name == "name" {
                        if let syn::Expr::Lit(lit) = &field.expr {
                            if let syn::Lit::Str(s) = &lit.lit {
                                name_val = Some(s.value());
                            }
                        }
                    } else if field_name == "func" {
                        // func is a path expression (identifier or path::to::fn)
                        func_val = Some(field.expr.to_token_stream().to_string());
                    }
                }
            }
            if let (Some(name), Some(func)) = (name_val, func_val) {
                let mut fields = BTreeMap::new();
                fields.insert(
                    TableKey::Keyword("name".into()),
                    Value::string(name.as_str()),
                );
                fields.insert(
                    TableKey::Keyword("func".into()),
                    Value::string(func.as_str()),
                );
                results.push(Value::struct_from(fields));
            }
        }
        syn::Expr::Array(a) => {
            for elem in &a.elems {
                collect_primitive_defs(elem, results);
            }
        }
        syn::Expr::Reference(r) => {
            collect_primitive_defs(&r.expr, results);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Serialization primitives (stubs)
// ---------------------------------------------------------------------------

fn prim_syn_to_string(args: &[Value]) -> (SignalBits, Value) {
    if let Some(file) = args[0].as_external::<syn::File>() {
        let s = file.to_token_stream().to_string();
        return (SIG_OK, Value::string(s.as_str()));
    }
    if let Some(item) = args[0].as_external::<syn::Item>() {
        let s = item.to_token_stream().to_string();
        return (SIG_OK, Value::string(s.as_str()));
    }
    if let Some(expr) = args[0].as_external::<syn::Expr>() {
        let s = expr.to_token_stream().to_string();
        return (SIG_OK, Value::string(s.as_str()));
    }
    if let Some(ty) = args[0].as_external::<syn::Type>() {
        let s = ty.to_token_stream().to_string();
        return (SIG_OK, Value::string(s.as_str()));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "syn/to-string: expected syn-file, syn-item, syn-expr, or syn-type, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn prim_syn_to_pretty_string(args: &[Value]) -> (SignalBits, Value) {
    if let Some(file) = args[0].as_external::<syn::File>() {
        let s = prettyplease::unparse(file);
        return (SIG_OK, Value::string(s.trim_end()));
    }
    if let Some(item) = args[0].as_external::<syn::Item>() {
        let file = syn::File {
            shebang: None,
            attrs: vec![],
            items: vec![item.clone()],
        };
        let s = prettyplease::unparse(&file);
        return (SIG_OK, Value::string(s.trim_end()));
    }
    (
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "syn/to-pretty-string: expected syn-file or syn-item, got {}",
                args[0].type_name()
            ),
        ),
    )
}

fn prim_syn_item_line(args: &[Value]) -> (SignalBits, Value) {
    let item = match args[0].as_external::<syn::Item>() {
        Some(i) => i,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "syn/item-line: expected syn-item, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match item_start_line(item) {
        Some(line) => (SIG_OK, Value::int(line as i64)),
        None => (SIG_OK, Value::NIL),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "syn/parse-file",
        func: prim_syn_parse_file,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a Rust source string into an opaque File node",
        params: &["source"],
        category: "syn",
        example: r#"(syn/parse-file "fn foo() {}")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/parse-expr",
        func: prim_syn_parse_expr,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a Rust expression string into an opaque Expr node",
        params: &["source"],
        category: "syn",
        example: r#"(syn/parse-expr "1 + 2")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/parse-type",
        func: prim_syn_parse_type,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a Rust type string into an opaque Type node",
        params: &["source"],
        category: "syn",
        example: r#"(syn/parse-type "Vec<String>")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/parse-item",
        func: prim_syn_parse_item,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse a Rust item string (fn, struct, enum, etc.) into an opaque Item node",
        params: &["source"],
        category: "syn",
        example: r#"(syn/parse-item "fn foo() {}")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/items",
        func: prim_syn_items,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract top-level items from a parsed file as a list of Item nodes",
        params: &["file"],
        category: "syn",
        example: r#"(syn/items (syn/parse-file "fn foo() {} fn bar() {}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/item-kind",
        func: prim_syn_item_kind,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the kind of an item as a keyword (:fn :struct :enum :trait :impl :use :mod :const :static :type :macro :other)",
        params: &["item"],
        category: "syn",
        example: r#"(syn/item-kind (syn/parse-item "fn foo() {}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/item-name",
        func: prim_syn_item_name,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the name (ident) of a named item as a string, or nil for unnamed items",
        params: &["item"],
        category: "syn",
        example: r#"(syn/item-name (syn/parse-item "fn foo() {}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/item-line",
        func: prim_syn_item_line,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the start line number of an item (1-indexed), or nil if unavailable",
        params: &["item"],
        category: "syn",
        example: r#"(syn/item-line (syn/parse-item "fn foo() {}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/fn-info",
        func: prim_syn_fn_info,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return {:name :args :return-type :async? :unsafe? :const?} for a function item",
        params: &["item"],
        category: "syn",
        example: r#"(syn/fn-info (syn/parse-item "pub fn add(x: i32) -> i32 { x }"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/fn-args",
        func: prim_syn_fn_args,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the argument list of a function item as ({:name string :type string} ...)",
        params: &["item"],
        category: "syn",
        example: r#"(syn/fn-args (syn/parse-item "fn foo(x: i32, y: String) {}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/fn-return-type",
        func: prim_syn_fn_return_type,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the return type of a function as a string, or nil if implicit ()",
        params: &["item"],
        category: "syn",
        example: r#"(syn/fn-return-type (syn/parse-item "fn foo() -> i32 { 42 }"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/struct-fields",
        func: prim_syn_struct_fields,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return {:name :kind :fields} for a struct item; :kind is :named :tuple or :unit",
        params: &["item"],
        category: "syn",
        example: r#"(syn/struct-fields (syn/parse-item "struct Foo { x: i32 }"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/enum-variants",
        func: prim_syn_enum_variants,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return {:name :variants} for an enum item; each variant has :name :kind :fields and optional :discriminant",
        params: &["item"],
        category: "syn",
        example: r#"(syn/enum-variants (syn/parse-item "enum Color { Red, Green, Blue }"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/attributes",
        func: prim_syn_attributes,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the attributes on an item as a list of strings",
        params: &["item"],
        category: "syn",
        example: r##"(syn/attributes (syn/parse-item "#[derive(Debug)] struct Foo {}"))"##,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/visibility",
        func: prim_syn_visibility,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the visibility of an item as a keyword (:public :pub-crate :pub-super :pub-in :private)",
        params: &["item"],
        category: "syn",
        example: r#"(syn/visibility (syn/parse-item "pub fn foo() {}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/to-string",
        func: prim_syn_to_string,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert any parsed syn node back to a compact token string",
        params: &["node"],
        category: "syn",
        example: r#"(syn/to-string (syn/parse-item "fn foo(){}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/to-pretty-string",
        func: prim_syn_to_pretty_string,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Pretty-print a parsed File or Item node using prettyplease",
        params: &["node"],
        category: "syn",
        example: r#"(syn/to-pretty-string (syn/parse-item "fn foo(){}"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/fn-calls",
        func: prim_syn_fn_calls,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract deduplicated function/method call names from a function body",
        params: &["item"],
        category: "syn",
        example: r#"(syn/fn-calls (syn/parse-item "fn foo() { bar(); baz::qux(); }"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/static-strings",
        func: prim_syn_static_strings,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract all string literals from a static/const item (e.g. PrimitiveDef arrays)",
        params: &["item"],
        category: "syn",
        example: r#"(syn/static-strings (syn/parse-item "static X: &[&str] = &[\"a\", \"b\"];"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "syn/primitive-defs",
        func: prim_syn_primitive_defs,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Extract name→func pairs from a PrimitiveDef const/static array",
        params: &["item"],
        category: "syn",
        example: r#"(syn/primitive-defs (syn/parse-item "const PRIMITIVES: &[PrimitiveDef] = &[...]"))"#,
        aliases: &[],
    },
];
