//! @string primitives (mutable strings)
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Create an @string from byte arguments
/// (@string) => empty @string
/// (@string 72 101 108) => @string with those bytes
pub(crate) fn prim_buffer(args: &[Value]) -> (SignalBits, Value) {
    let mut bytes = Vec::with_capacity(args.len());
    for (i, arg) in args.iter().enumerate() {
        match arg.as_int() {
            Some(n) if (0..=255).contains(&n) => bytes.push(n as u8),
            Some(n) => {
                return (
                    SIG_ERROR,
                    error_val(
                        "error",
                        format!("buffer: byte {} out of range 0-255: {}", i, n),
                    ),
                )
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "buffer: expected integer, got {} at position {}",
                            arg.type_name(),
                            i
                        ),
                    ),
                )
            }
        }
    }
    (SIG_OK, Value::string_mut(bytes))
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[PrimitiveDef {
    name: "@string",
    func: prim_buffer,
    effect: Effect::inert(),
    arity: Arity::AtLeast(0),
    doc: "Create a mutable string from byte arguments.",
    params: &[],
    category: "buffer",
    example: "(@string 72 101 108 108 111)",
    aliases: &[],
}];
