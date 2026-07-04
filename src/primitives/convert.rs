//! Type conversion primitives
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

mod numeric;
mod tostring;
pub(crate) use numeric::{
    prim_number_to_string, prim_parse_float, prim_parse_int, prim_to_float, prim_to_int,
};
pub(crate) use tostring::prim_to_string;

// Declarative primitive definitions for conversion module.
primitive! {
    "integer" => prim_to_int {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert number to integer (i64). Accepts int (identity) or float (truncation). Use parse-int for string→int.",
        params: &["x"],
        category: "conversion",
        example: "(integer 3.7) #=> 3\n(integer 42) #=> 42",
        aliases: &["int"],
        effect: RegionEffect::Immediate,
    }
    "float" => prim_to_float {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert number to float. Accepts int (→ f64) or float (identity). Use parse-float for string→float.",
        params: &["x"],
        category: "conversion",
        example: "(float 42) #=> 42.0\n(float 3.14) #=> 3.14",
        effect: RegionEffect::Immediate,
    }
    "parse-int" => prim_parse_int {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Parse string or keyword to integer. Optional radix (2–36) for base conversion.",
        params: &["s", "radix?"],
        category: "conversion",
        example: "(parse-int \"42\") #=> 42\n(parse-int \"ff\" 16) #=> 255\n(parse-int \"1010\" 2) #=> 10",
        effect: RegionEffect::Immediate,
    }
    "parse-float" => prim_parse_float {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse string or keyword to float.",
        params: &["s"],
        category: "conversion",
        example: "(parse-float \"3.14\") #=> 3.14",
        effect: RegionEffect::Immediate,
    }
    "string" => prim_to_string {
        ret: RetType::String,
        signal: Signal::errors(),
        arity: Arity::AtLeast(0),
        doc: "Convert values to string. Multiple arguments are concatenated.",
        params: &["values"],
        category: "conversion",
        example: "(string \"count: \" 42) #=> \"count: 42\"",
        aliases: &["any->string", "symbol->string"],
        // `string` READS its arguments and returns a FRESH string (every path now
        // allocates in the call's own region — see `prim_to_string_single`); it never
        // STORES an argument. `Mixed` would tell the escape analysis it may store
        // every heap arg (the mutual clique), emitting an arg escape-incref the native
        // never balances — one leaked region per heap arg
        // (tests/elle/region-string-concat-leak.lisp). `Fresh` is the truthful effect.
        effect: RegionEffect::Fresh,
    }
    "number->string" => prim_number_to_string {
        ret: RetType::String,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Convert a number to string. With an optional radix (2–36), converts an integer to the given base (lowercase, no prefix).",
        params: &["n", "radix?"],
        category: "conversion",
        example: "(number->string 42) #=> \"42\"\n(number->string 255 16) #=> \"ff\"\n(number->string -255 16) #=> \"-ff\"",
        effect: RegionEffect::Fresh,
    }
}
