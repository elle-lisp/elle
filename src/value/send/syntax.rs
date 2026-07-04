// ── sendable syntax ──────────────────────────────────────────────────────
//
// A Send-safe mirror of `crate::syntax::Syntax` (the pre-analysis syntax tree).
// Lets parsed source cross `os/spawn`: the test runner reads a legacy multi-form
// file in the main VM and ships the syntax to a worker, which compiles + runs it
// with its OWN stdlib so the file's runtime `import`s and the worker's `ev/run`
// scheduler share one set of dynamic parameters. Spans (error locations) and
// hygiene scopes are preserved. `SyntaxKind::SyntaxLiteral` — a macro-template
// symbol embedded by quasiquote, never produced by the reader — is the only
// unsendable case, so this type stays self-contained (no `SendValue` inside, no
// context).

use crate::syntax::{ScopeId, Span, Syntax, SyntaxKind};

/// Discriminant for the homogeneous `Vec<Syntax>` compound kinds.
#[derive(Clone, Copy)]
enum SeqKind {
    List,
    Array,
    ArrayMut,
    Struct,
    StructMut,
    Set,
    SetMut,
    Bytes,
    BytesMut,
}

/// Discriminant for the single-child `Box<Syntax>` quote kinds.
#[derive(Clone, Copy)]
enum WrapKind {
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    Splice,
}

#[derive(Clone)]
enum SendSyntaxKind {
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(String),
    Keyword(String),
    Str(String),
    StrMut(String),
    Seq(SeqKind, Vec<SendSyntax>),
    Wrap(WrapKind, Box<SendSyntax>),
}

#[derive(Clone)]
pub struct SendSyntax {
    kind: SendSyntaxKind,
    span: Span,
    scopes: Vec<u32>,
    scope_exempt: bool,
}

fn seq_to_send(xs: &[Syntax]) -> Result<Vec<SendSyntax>, String> {
    xs.iter().map(syntax_to_send).collect()
}

pub(super) fn syntax_to_send(s: &Syntax) -> Result<SendSyntax, String> {
    let kind = match &s.kind {
        SyntaxKind::Nil => SendSyntaxKind::Nil,
        SyntaxKind::Bool(b) => SendSyntaxKind::Bool(*b),
        SyntaxKind::Int(i) => SendSyntaxKind::Int(*i),
        SyntaxKind::Float(f) => SendSyntaxKind::Float(*f),
        SyntaxKind::Symbol(n) => SendSyntaxKind::Symbol(n.clone()),
        SyntaxKind::Keyword(n) => SendSyntaxKind::Keyword(n.clone()),
        SyntaxKind::String(n) => SendSyntaxKind::Str(n.clone()),
        SyntaxKind::StringMut(n) => SendSyntaxKind::StrMut(n.clone()),
        SyntaxKind::List(xs) => SendSyntaxKind::Seq(SeqKind::List, seq_to_send(xs)?),
        SyntaxKind::Array(xs) => SendSyntaxKind::Seq(SeqKind::Array, seq_to_send(xs)?),
        SyntaxKind::ArrayMut(xs) => SendSyntaxKind::Seq(SeqKind::ArrayMut, seq_to_send(xs)?),
        SyntaxKind::Struct(xs) => SendSyntaxKind::Seq(SeqKind::Struct, seq_to_send(xs)?),
        SyntaxKind::StructMut(xs) => SendSyntaxKind::Seq(SeqKind::StructMut, seq_to_send(xs)?),
        SyntaxKind::Set(xs) => SendSyntaxKind::Seq(SeqKind::Set, seq_to_send(xs)?),
        SyntaxKind::SetMut(xs) => SendSyntaxKind::Seq(SeqKind::SetMut, seq_to_send(xs)?),
        SyntaxKind::Bytes(xs) => SendSyntaxKind::Seq(SeqKind::Bytes, seq_to_send(xs)?),
        SyntaxKind::BytesMut(xs) => SendSyntaxKind::Seq(SeqKind::BytesMut, seq_to_send(xs)?),
        SyntaxKind::Quote(x) => SendSyntaxKind::Wrap(WrapKind::Quote, Box::new(syntax_to_send(x)?)),
        SyntaxKind::Quasiquote(x) => {
            SendSyntaxKind::Wrap(WrapKind::Quasiquote, Box::new(syntax_to_send(x)?))
        }
        SyntaxKind::Unquote(x) => {
            SendSyntaxKind::Wrap(WrapKind::Unquote, Box::new(syntax_to_send(x)?))
        }
        SyntaxKind::UnquoteSplicing(x) => {
            SendSyntaxKind::Wrap(WrapKind::UnquoteSplicing, Box::new(syntax_to_send(x)?))
        }
        SyntaxKind::Splice(x) => {
            SendSyntaxKind::Wrap(WrapKind::Splice, Box::new(syntax_to_send(x)?))
        }
        SyntaxKind::SyntaxLiteral(_) => {
            return Err(
                "Cannot send syntax with an embedded value literal (post-expansion syntax)"
                    .to_string(),
            )
        }
    };
    Ok(SendSyntax {
        kind,
        span: s.span.clone(),
        scopes: s.scopes.iter().map(|sc| sc.0).collect(),
        scope_exempt: s.scope_exempt,
    })
}

pub(super) fn send_to_syntax(ss: SendSyntax) -> Syntax {
    let kind = match ss.kind {
        SendSyntaxKind::Nil => SyntaxKind::Nil,
        SendSyntaxKind::Bool(b) => SyntaxKind::Bool(b),
        SendSyntaxKind::Int(i) => SyntaxKind::Int(i),
        SendSyntaxKind::Float(f) => SyntaxKind::Float(f),
        SendSyntaxKind::Symbol(n) => SyntaxKind::Symbol(n),
        SendSyntaxKind::Keyword(n) => SyntaxKind::Keyword(n),
        SendSyntaxKind::Str(n) => SyntaxKind::String(n),
        SendSyntaxKind::StrMut(n) => SyntaxKind::StringMut(n),
        SendSyntaxKind::Seq(sk, xs) => {
            let items: Vec<Syntax> = xs.into_iter().map(send_to_syntax).collect();
            match sk {
                SeqKind::List => SyntaxKind::List(items),
                SeqKind::Array => SyntaxKind::Array(items),
                SeqKind::ArrayMut => SyntaxKind::ArrayMut(items),
                SeqKind::Struct => SyntaxKind::Struct(items),
                SeqKind::StructMut => SyntaxKind::StructMut(items),
                SeqKind::Set => SyntaxKind::Set(items),
                SeqKind::SetMut => SyntaxKind::SetMut(items),
                SeqKind::Bytes => SyntaxKind::Bytes(items),
                SeqKind::BytesMut => SyntaxKind::BytesMut(items),
            }
        }
        SendSyntaxKind::Wrap(wk, x) => {
            let inner = Box::new(send_to_syntax(*x));
            match wk {
                WrapKind::Quote => SyntaxKind::Quote(inner),
                WrapKind::Quasiquote => SyntaxKind::Quasiquote(inner),
                WrapKind::Unquote => SyntaxKind::Unquote(inner),
                WrapKind::UnquoteSplicing => SyntaxKind::UnquoteSplicing(inner),
                WrapKind::Splice => SyntaxKind::Splice(inner),
            }
        }
    };
    Syntax {
        kind,
        span: ss.span,
        scopes: ss.scopes.into_iter().map(ScopeId).collect(),
        scope_exempt: ss.scope_exempt,
    }
}
