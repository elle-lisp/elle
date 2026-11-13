// audited: 2026-11-13
//! What a call hands a closure callee: the offers its bytecode carries, the positions the callee takes, and each parameter's reference.
//!
//! docs/impl/region/handoff.md

use crate::lir::Offer;
use crate::value::{Arity, Closure, Value, VarargTag};

/// A set of argument positions, each below 64.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Positions(u64);

impl Positions {
    fn contains(self, position: usize) -> bool {
        position < 64 && self.0 & (1 << position) != 0
    }

    fn with(self, position: usize) -> Positions {
        debug_assert!(position < 64, "an offer names a position below 64");
        Positions(self.0 | (1 << position))
    }
}

/// The positions a call offered to move into its callee.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Offered(Positions);

/// The positions a callee took: offered, and filling an owned parameter. The
/// frame stamps each taken argument's slot `nil`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Taken(Positions);

impl Offered {
    #[cfg(feature = "jit")]
    pub(crate) const NONE: Offered = Offered(Positions(0));

    /// The positions compiled code hands `elle_jit_call` in its hand-off word.
    #[cfg(feature = "jit")]
    pub(crate) fn from_word(word: u64) -> Offered {
        Offered(Positions(word))
    }

    /// The offers `closure` takes for `args`: those that fill a fixed
    /// parameter the closure owns. A captured parameter is held by its cell,
    /// and a rest argument by the collection.
    pub(crate) fn take(self, closure: &Closure, args: &[Value]) -> Taken {
        let fixed = fixed_arg_count(closure, args).min(64);
        let taken = (0..fixed)
            .filter(|&i| self.0.contains(i) && !param_is_celled(closure, i))
            .fold(Positions::default(), Positions::with);
        Taken(taken)
    }
}

impl Taken {
    pub(crate) fn contains(self, position: usize) -> bool {
        self.0.contains(position)
    }

    /// The word a JIT call helper writes back to the caller's hand-off word.
    #[cfg(feature = "jit")]
    pub(crate) fn word(self) -> u64 {
        self.0 .0
    }
}

/// The offers a call instruction carries in the bytecode: a count byte, then a
/// position byte and a big-endian `u16` slot each (docs/impl/bytecode.md).
#[derive(Clone, Copy)]
pub(crate) struct CodeOffers<'a>(&'a [u8]);

impl<'a> CodeOffers<'a> {
    /// A call that offers nothing.
    pub(crate) const NONE: CodeOffers<'static> = CodeOffers(&[]);

    /// Read the offers at `ip`, and step `ip` past them.
    pub(crate) fn read(bytecode: &'a [u8], ip: &mut usize) -> CodeOffers<'a> {
        let start = *ip + 1;
        *ip = start + 3 * bytecode[*ip] as usize;
        CodeOffers(&bytecode[start..*ip])
    }

    pub(crate) fn iter(self) -> impl Iterator<Item = Offer> + 'a {
        self.0.chunks_exact(3).map(|o| Offer {
            position: o[0],
            slot: u16::from_be_bytes([o[1], o[2]]),
        })
    }

    pub(crate) fn offered(self) -> Offered {
        Offered(
            self.iter()
                .fold(Positions::default(), |p, o| p.with(o.position as usize)),
        )
    }

    /// Stamp `nil` into the slot of each offer the callee took, in the frame
    /// whose locals start at `base` on `stack`. The frame's release of each
    /// such slot then loads an immediate and does nothing.
    pub(crate) fn discharge(self, taken: Taken, stack: &mut [Value], base: usize) {
        for offer in self.iter().filter(|o| taken.contains(o.position as usize)) {
            stack[base + offer.slot as usize] = Value::NIL;
        }
    }
}

/// How many of `args` fill `closure`'s fixed parameters. The rest are
/// collected into its rest parameter. A keyword collector fills an optional
/// parameter only with a non-keyword argument.
pub(crate) fn fixed_arg_count(closure: &Closure, args: &[Value]) -> usize {
    let Arity::AtLeast(min) = closure.template.arity() else {
        return args.len();
    };
    let fixed_slots = closure.template.num_params() - 1;
    let collects_keywords = matches!(
        closure.template.vararg_tag(),
        VarargTag::Struct | VarargTag::StrictStruct
    );
    if !collects_keywords {
        return args.len().min(fixed_slots);
    }
    let mut count = args.len().min(min);
    while count < fixed_slots && count < args.len() && !args[count].is_keyword() {
        count += 1;
    }
    count
}

/// Is `closure`'s parameter `i` held by a capture cell rather than owned?
pub(crate) fn param_is_celled(closure: &Closure, i: usize) -> bool {
    i < 64 && closure.template.capture_params_mask() & (1 << i) != 0
}

/// The reference a callee's environment takes for each owned parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ArgRefs {
    /// A fresh one per argument: a call, or a spliced call in either position.
    Mint,
    /// The frame's own: a tail call, or an FFI callback.
    Move,
    /// The frame's own for each taken position, and a fresh one for the rest.
    MintExcept(Taken),
}

impl ArgRefs {
    /// The references a tail call hands: the frame's own, or a fresh one each
    /// where the call is spliced and the frame holds none to move.
    pub(crate) fn tail(spliced: bool) -> ArgRefs {
        if spliced {
            ArgRefs::Mint
        } else {
            ArgRefs::Move
        }
    }

    /// Does the parameter at `position` take a fresh reference?
    pub(crate) fn mints(self, position: usize) -> bool {
        match self {
            ArgRefs::Mint => true,
            ArgRefs::Move => false,
            ArgRefs::MintExcept(taken) => !taken.contains(position),
        }
    }
}
