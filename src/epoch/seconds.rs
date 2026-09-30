// audited: 2026-09-30
//! What epoch 14 makes of a duration written in milliseconds: the one conversion the compiler and `elle rewrite` share.
//!
//! docs/epochs.md
//! docs/io/timeout.md

use super::rules::TimeArg;
use std::fmt;

/// The wrap an expression that computed milliseconds gets: the same value in
/// seconds at run time, with `nil` still meaning no bound. `$1` stands for the
/// expression.
const DIVIDE: &str = "(if-let [ms $1] (/ ms 1000.0) nil)";

/// The wrap for a call that read a negative duration as no bound.
const DIVIDE_OR_UNBOUNDED: &str = "(if-let [ms $1] (if (< ms 0) nil (/ ms 1000.0)) nil)";

/// A duration argument as the source wrote it, told apart by the shapes the
/// conversion treats differently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Millis {
    Int(i64),
    Float(f64),
    Nil,
    /// Anything else: an expression whose value is known only at run time.
    Expr,
}

/// A number of seconds, as the literal the conversion writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SecondsLiteral {
    Int(i64),
    Float(f64),
}

impl fmt::Display for SecondsLiteral {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SecondsLiteral::Int(n) => write!(f, "{n}"),
            // Debug prints the shortest text that reads back as the same
            // float, and always with a point: `0.5`, `5.0`.
            SecondsLiteral::Float(x) => write!(f, "{x:?}"),
        }
    }
}

/// What a duration argument becomes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seconds {
    /// Leave the argument as written: a keyword's `nil` already means no bound.
    Keep,
    /// Drop the argument: it meant no bound, and in seconds no bound is no
    /// argument at all.
    Drop,
    /// Write the duration anew.
    Write(Written),
}

/// A duration written in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Written {
    /// This literal, in place of the one the source had.
    Literal(SecondsLiteral),
    /// The expression wrapped in this template, where `$1` stands for it.
    Wrap(&'static str),
}

impl Seconds {
    /// The conversion of `ms`, given where the call took it.
    pub fn of(ms: Millis, place: TimeArg) -> Seconds {
        let unbounded_below = matches!(
            place,
            TimeArg::Last {
                negative_unbounded: true,
                ..
            }
        );
        let literal = |s| Seconds::Write(Written::Literal(s));
        match ms {
            Millis::Nil if place == TimeArg::Keyword => Seconds::Keep,
            Millis::Nil => Seconds::Drop,
            Millis::Int(n) if unbounded_below && n < 0 => Seconds::Drop,
            Millis::Float(x) if unbounded_below && x < 0.0 => Seconds::Drop,
            Millis::Int(n) if n % 1000 == 0 => literal(SecondsLiteral::Int(n / 1000)),
            Millis::Int(n) => literal(SecondsLiteral::Float(n as f64 / 1000.0)),
            Millis::Float(x) => literal(SecondsLiteral::Float(x / 1000.0)),
            Millis::Expr if unbounded_below => Seconds::Write(Written::Wrap(DIVIDE_OR_UNBOUNDED)),
            Millis::Expr => Seconds::Write(Written::Wrap(DIVIDE)),
        }
    }
}

/// The text a wrap template puts before its expression and after it.
pub fn wrap_ends(template: &'static str) -> (&'static str, &'static str) {
    template
        .split_once("$1")
        .expect("a wrap template names its expression once")
}
