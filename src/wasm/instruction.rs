//! LIR instruction → WASM instruction emission.
//!
//! Translates individual LIR instructions into WASM bytecode via
//! `wasm-encoder`. Covers arithmetic, comparisons, data operations,
//! calls, tail calls, constants, and memory access helpers.
//!
//! The per-instruction dispatch lives in [`dispatch`]; the concrete emitters
//! are grouped by concern across [`calls`] (closures/calls/tail calls),
//! [`data`] (`rt_data_op` helpers), [`arith`] (inline numeric/comparison), and
//! [`mem`] (memory marshalling + constant materialization).

use crate::lir::{BinOp, CmpOp, LirConst, LirInstr, Reg, UnaryOp};
use crate::value::repr::*;
use crate::value::Value;
use wasm_encoder::*;

use super::emit::*;

mod arith;
mod calls;
mod data;
mod dispatch;
mod mem;
