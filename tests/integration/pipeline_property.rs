// Property-based tests for the new Syntax → HIR → LIR compilation pipeline
//
// These tests verify semantic correctness by checking mathematical properties
// hold when code is compiled and executed through the new pipeline.
//
// This file is a coordinator: shared imports live here, and the test blocks
// are split into themed subfiles wired in via `include!`. The subfiles use
// `use super::*;` so the imports below are visible inside them.

use crate::common::eval_reuse;
use elle::Value;
use proptest::prelude::*;

mod arithmetic {
    include!("pipeline_property/arithmetic.rs");
}
mod collections {
    include!("pipeline_property/collections.rs");
}
mod closures {
    include!("pipeline_property/closures.rs");
}
mod control {
    include!("pipeline_property/control.rs");
}
mod recursion {
    include!("pipeline_property/recursion.rs");
}
mod higher {
    include!("pipeline_property/higher.rs");
}
mod letrec {
    include!("pipeline_property/letrec.rs");
}
