// audited: 2026-11-21
//! A native's signature in the vocabulary of signatures: one mode per parameter, and one kind of result.
//!
//! docs/spec/signature.md
//! docs/spec/form.md

use crate::value::types::Arity;

/// What a native does with the argument at one parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reads the value and keeps nothing.
    Borrowed,
    /// Stores the value into the place.
    Stored(Place),
    /// Gives the result the value: an alias or a fresh result borrows from
    /// it, and an itself result is it.
    Returned,
    /// Passes the value to another fiber.
    Crosses,
    /// Resumes the fiber the value is.
    Resumed,
}

/// Where a stored parameter's value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The argument at another parameter.
    Param(usize),
    /// The native's fresh result.
    Result,
}

/// What a native's result is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// No heap value.
    Immediate,
    /// A value the call made.
    Fresh,
    /// A value borrowed from each returned argument.
    Alias,
    /// The argument at this parameter itself.
    Itself(usize),
    /// The payload of the fiber at this parameter.
    Payload(usize),
}

/// A native's declared signature: a mode for each fixed parameter, a mode
/// for each argument past them, and the kind of the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    params: &'static [Mode],
    rest: Option<Mode>,
    result: Kind,
}

impl Spec {
    /// The signature of a native whose arity is a fixed count.
    pub const fn fixed(params: &'static [Mode], result: Kind) -> Spec {
        Spec {
            params,
            rest: None,
            result,
        }
    }

    /// The signature of a native whose arity has no fixed count: `rest` is
    /// the mode of each argument past `params`.
    pub const fn rest(params: &'static [Mode], rest: Mode, result: Kind) -> Spec {
        Spec {
            params,
            rest: Some(rest),
            result,
        }
    }

    /// The mode of each fixed parameter.
    pub fn params(&self) -> &'static [Mode] {
        self.params
    }

    /// The mode of each argument past the fixed parameters, if any.
    pub fn rest_mode(&self) -> Option<Mode> {
        self.rest
    }

    /// The kind of the result.
    pub fn result(&self) -> Kind {
        self.result
    }

    /// Whether the declaration keeps the rules of a native's signature for a
    /// native of `arity`, or the rule it breaks.
    pub fn check(&self, arity: Arity) -> Result<(), String> {
        let (count, fixed) = match arity {
            Arity::Exact(n) => (n, true),
            Arity::AtLeast(n) | Arity::Range(n, _) => (n, false),
        };
        if self.params.len() != count {
            return Err(format!(
                "{} modes for {count} fixed parameters",
                self.params.len()
            ));
        }
        if fixed != self.rest.is_none() {
            return Err("a rest mode belongs to an arity with no fixed count".into());
        }
        let modes = || self.params.iter().chain(self.rest.iter());
        for (i, mode) in modes().enumerate() {
            match mode {
                Mode::Stored(Place::Param(j)) if *j >= count || *j == i => {
                    return Err(format!(
                        "a store names parameter {j}, not another fixed one"
                    ))
                }
                Mode::Stored(Place::Result) if self.result != Kind::Fresh => {
                    return Err("a store into the result needs a fresh result".into())
                }
                _ => {}
            }
        }
        let returned = modes().filter(|m| **m == Mode::Returned).count();
        match self.result {
            Kind::Immediate | Kind::Payload(_) if returned > 0 => {
                Err("a returned parameter needs a result that borrows".into())
            }
            Kind::Alias if returned == 0 => Err("an alias borrows from no parameter".into()),
            Kind::Itself(i) if self.params.get(i) != Some(&Mode::Returned) || returned != 1 => Err(
                format!("itself names parameter {i}, not the one returned parameter"),
            ),
            Kind::Payload(i) if i >= count => {
                Err(format!("a payload names parameter {i}, not a fixed one"))
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
