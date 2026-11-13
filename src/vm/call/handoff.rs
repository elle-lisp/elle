// audited: 2026-11-13
//! What a call hands a closure callee: the offers its bytecode carries, the positions the callee takes, and each parameter's reference.
//!
//! docs/impl/region/handoff.md

use crate::lir::Offer;
use crate::value::arena::{incref_for_escape, result_region_of, EscapeSite};
use crate::value::fiberheap::FiberHeap;
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

    /// The argument at each position, among `args`.
    fn of(self, args: &[Value]) -> impl Iterator<Item = Value> + '_ {
        args.iter()
            .take(64)
            .enumerate()
            .filter(move |(i, _)| self.contains(*i))
            .map(|(_, &arg)| arg)
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

    /// The word a JIT tail-call helper writes back: the late mints it did not
    /// make, whose stashes the frame stamps `nil`.
    #[cfg(feature = "jit")]
    pub(crate) fn word(self) -> u64 {
        self.0 .0
    }

    /// These positions, less `other`'s.
    #[cfg(feature = "jit")]
    pub(crate) fn without(self, other: Offered) -> Offered {
        Offered(Positions(self.0 .0 & !other.0 .0))
    }

    /// Mint, for the argument at each position, the reference a borrowed tail
    /// argument hands its callee: a late mint, made once the callee is known.
    /// Answers the values minted, for a call that must give the mints back.
    pub(crate) fn mint(self, heap: &mut FiberHeap, args: &[Value]) -> Minted {
        let minted: Vec<Value> = self.0.of(args).collect();
        for &arg in &minted {
            let region = result_region_of(heap, arg);
            incref_for_escape(heap, region, EscapeSite::CallArgument);
        }
        Minted(minted)
    }
}

/// The values a late mint counted, held until the call is certain to hand
/// them to a callee.
#[must_use]
pub(crate) struct Minted(Vec<Value>);

impl Minted {
    /// Give back what [`Offered::mint`] took, for a closure callee whose
    /// environment refused its arguments, so no callee runs to release them.
    pub(crate) fn unmint(self, heap: &mut FiberHeap) {
        for arg in self.0 {
            if let Some(region) = result_region_of(heap, arg) {
                heap.decref_region(region);
            }
        }
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

    /// Write the argument at each offer's position back into its slot, in the
    /// frame whose locals start at `base` on `stack`.
    pub(super) fn restore(self, args: &[Value], stack: &mut [Value], base: usize) {
        for offer in self.iter() {
            stack[base + offer.slot as usize] = args[offer.position as usize];
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
    /// The frame's own: a tail call, or an FFI callback. The trampoline's call
    /// of a compiled tail callee takes none either, because the tail call's
    /// resolver took every reference the callee owns.
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
