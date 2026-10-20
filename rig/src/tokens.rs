// audited: 2026-10-20
//! The `elle/tokens` primitive: what a text costs a reader, in tokens of the `o200k_base` encoding.
//!
//! rig/overview.md

use elle::primitives::ctx::NativeCtx;
use elle::primitives::def::{PrimitiveDef, RegionEffect};
use elle::runtime::Runtime;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::Value;

/// The tokens of `text`, every byte read as ordinary text. A special token's
/// spelling is text a reader pays for like any other, so it is never read as
/// the one token the encoding reserves for it.
fn count(text: &str) -> usize {
    tiktoken_rs::o200k_base_singleton()
        .encode_ordinary(text)
        .len()
}

fn prim_tokens(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match args[0].with_string(count) {
        Some(n) => (SIG_OK, Value::int(n as i64)),
        None => (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("elle/tokens: expected string, got {}", args[0].type_name()),
            ),
        ),
    }
}

static TOKENS: PrimitiveDef = PrimitiveDef {
    name: "elle/tokens",
    func: prim_tokens,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Return how many tokens TEXT costs a reader in the `o200k_base` encoding. The spelling of a special token counts as the text it is (rig/overview.md).",
    params: &["text"],
    category: "elle",
    example: "(elle/tokens \"hello world\")",
    effect: RegionEffect::Immediate,
    ..PrimitiveDef::DEFAULT
};

/// Register `elle/tokens` on `rt`, for compiled code and `eval` alike.
pub fn register(rt: &mut Runtime) {
    rt.register_primitive(&TOKENS);
}
