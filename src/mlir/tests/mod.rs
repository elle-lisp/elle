use super::lower;
use super::spirv;
use super::*;
use crate::lir::*;
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::Arity;

fn s() -> Span {
    Span::synthetic()
}

mod abs;
mod bench;
mod capture;
mod compare;
mod convert;
mod lowering;
mod spvgen;
mod typecheck;
