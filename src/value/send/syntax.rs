// ── sendable syntax ──────────────────────────────────────────────────────
//
// A Send-safe mirror of `crate::syntax::Syntax` (the pre-analysis syntax tree).
// Lets parsed source cross `os/spawn`: the test runner reads a legacy multi-form
// file in the main VM and ships the syntax to a worker, which compiles + runs it
// with its OWN stdlib so the file's runtime `import`s and the worker's `ev/run`
// scheduler share one set of dynamic parameters. Spans (error locations) and
// hygiene scopes are preserved.
//
// The mirror exists because a `Syntax` node's payloads are pointers into a
// region, and a region belongs to one `RegionStore` (docs/impl/syntax.md).
// Crossing a thread therefore means owning the payloads on the way out and
// rebuilding them in the receiver's arena on the way in — the same trade a
// string makes. Every kind crosses, `SyntaxLiteral` included: it is an ordinary
// single-child node now, not a heap `Value` the mirror had to refuse.

use crate::syntax::{ScopeId, Span, SynRef, Syntax, SyntaxArena, SyntaxKind};

/// Discriminant for the homogeneous sequence kinds.
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

impl SeqKind {
    fn rebuild(self, arena: &SyntaxArena, items: &[Syntax]) -> SyntaxKind {
        let slice = arena.nodes(items);
        match self {
            SeqKind::List => SyntaxKind::List(slice),
            SeqKind::Array => SyntaxKind::Array(slice),
            SeqKind::ArrayMut => SyntaxKind::ArrayMut(slice),
            SeqKind::Struct => SyntaxKind::Struct(slice),
            SeqKind::StructMut => SyntaxKind::StructMut(slice),
            SeqKind::Set => SyntaxKind::Set(slice),
            SeqKind::SetMut => SyntaxKind::SetMut(slice),
            SeqKind::Bytes => SyntaxKind::Bytes(slice),
            SeqKind::BytesMut => SyntaxKind::BytesMut(slice),
        }
    }
}

/// Discriminant for the single-child kinds.
#[derive(Clone, Copy)]
enum WrapKind {
    Quote,
    Quasiquote,
    Unquote,
    UnquoteSplicing,
    Splice,
    Literal,
}

impl WrapKind {
    fn rebuild(self, inner: SynRef) -> SyntaxKind {
        match self {
            WrapKind::Quote => SyntaxKind::Quote(inner),
            WrapKind::Quasiquote => SyntaxKind::Quasiquote(inner),
            WrapKind::Unquote => SyntaxKind::Unquote(inner),
            WrapKind::UnquoteSplicing => SyntaxKind::UnquoteSplicing(inner),
            WrapKind::Splice => SyntaxKind::Splice(inner),
            WrapKind::Literal => SyntaxKind::SyntaxLiteral(inner),
        }
    }
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

/// The sequence discriminant for a kind, or `None` if it is not a sequence.
fn seq_kind_of(kind: &SyntaxKind) -> Option<SeqKind> {
    Some(match kind {
        SyntaxKind::List(_) => SeqKind::List,
        SyntaxKind::Array(_) => SeqKind::Array,
        SyntaxKind::ArrayMut(_) => SeqKind::ArrayMut,
        SyntaxKind::Struct(_) => SeqKind::Struct,
        SyntaxKind::StructMut(_) => SeqKind::StructMut,
        SyntaxKind::Set(_) => SeqKind::Set,
        SyntaxKind::SetMut(_) => SeqKind::SetMut,
        SyntaxKind::Bytes(_) => SeqKind::Bytes,
        SyntaxKind::BytesMut(_) => SeqKind::BytesMut,
        _ => return None,
    })
}

/// The single-child discriminant for a kind, with the child, or `None` if it
/// is not a wrapping kind.
fn wrap_kind_of(kind: &SyntaxKind) -> Option<(WrapKind, &Syntax)> {
    Some(match kind {
        SyntaxKind::Quote(x) => (WrapKind::Quote, x),
        SyntaxKind::Quasiquote(x) => (WrapKind::Quasiquote, x),
        SyntaxKind::Unquote(x) => (WrapKind::Unquote, x),
        SyntaxKind::UnquoteSplicing(x) => (WrapKind::UnquoteSplicing, x),
        SyntaxKind::Splice(x) => (WrapKind::Splice, x),
        SyntaxKind::SyntaxLiteral(x) => (WrapKind::Literal, x),
        _ => return None,
    })
}

pub(super) fn syntax_to_send(s: &Syntax) -> Result<SendSyntax, String> {
    let kind = match &s.kind {
        SyntaxKind::Nil => SendSyntaxKind::Nil,
        SyntaxKind::Bool(b) => SendSyntaxKind::Bool(*b),
        SyntaxKind::Int(i) => SendSyntaxKind::Int(*i),
        SyntaxKind::Float(f) => SendSyntaxKind::Float(*f),
        SyntaxKind::Symbol(n) => SendSyntaxKind::Symbol(n.to_string()),
        SyntaxKind::Keyword(n) => SendSyntaxKind::Keyword(n.to_string()),
        SyntaxKind::String(n) => SendSyntaxKind::Str(n.to_string()),
        SyntaxKind::StringMut(n) => SendSyntaxKind::StrMut(n.to_string()),
        other => {
            if let Some(seq) = seq_kind_of(other) {
                let items = other
                    .children()
                    .iter()
                    .map(syntax_to_send)
                    .collect::<Result<Vec<_>, _>>()?;
                SendSyntaxKind::Seq(seq, items)
            } else {
                // `wrap_kind_of` covers every remaining variant, so the
                // `expect` states an exhaustiveness fact rather than a hope:
                // an atom was matched above and a sequence just now.
                let (wrap, inner) =
                    wrap_kind_of(other).expect("every non-atom, non-sequence kind wraps one child");
                SendSyntaxKind::Wrap(wrap, Box::new(syntax_to_send(inner)?))
            }
        }
    };
    Ok(SendSyntax {
        kind,
        span: s.span,
        scopes: s.scopes().iter().map(|sc| sc.0).collect(),
        scope_exempt: s.scope_exempt,
    })
}

pub(super) fn send_to_syntax(arena: &SyntaxArena, ss: SendSyntax) -> Syntax {
    let kind = match ss.kind {
        SendSyntaxKind::Nil => SyntaxKind::Nil,
        SendSyntaxKind::Bool(b) => SyntaxKind::Bool(b),
        SendSyntaxKind::Int(i) => SyntaxKind::Int(i),
        SendSyntaxKind::Float(f) => SyntaxKind::Float(f),
        SendSyntaxKind::Symbol(n) => SyntaxKind::Symbol(arena.text(&n)),
        SendSyntaxKind::Keyword(n) => SyntaxKind::Keyword(arena.text(&n)),
        SendSyntaxKind::Str(n) => SyntaxKind::String(arena.text(&n)),
        SendSyntaxKind::StrMut(n) => SyntaxKind::StringMut(arena.text(&n)),
        SendSyntaxKind::Seq(sk, xs) => {
            let items: Vec<Syntax> = xs.into_iter().map(|x| send_to_syntax(arena, x)).collect();
            sk.rebuild(arena, &items)
        }
        SendSyntaxKind::Wrap(wk, x) => wk.rebuild(arena.node(send_to_syntax(arena, *x))),
    };
    let mut out = Syntax::with_scopes(
        arena,
        kind,
        ss.span,
        &ss.scopes.into_iter().map(ScopeId).collect::<Vec<_>>(),
    );
    out.scope_exempt = ss.scope_exempt;
    out
}
