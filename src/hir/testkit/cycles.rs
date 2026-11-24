// audited: 2026-11-24
//! Source fragments in which one tail call strands two closure-cycle arenas, for the tests of every layer that carries them.
//!
//! docs/impl/region/cycletail.md

/// A function `f` whose one `letrec` binds two mutual cycles, `a1`/`a2` and
/// `b1`/`b2`, and whose body ends in a branch with one arm a tail call to the
/// foreign `g`. That call is a non-member tail of both cycles, so it strands
/// both merged arenas. The source defines `g` and `f` and calls neither. A
/// coerce proves `k` an integer, so `f` compiles where it is used as a value and
/// no call site proves its argument. It names no stdlib function, because a
/// [`super::HirFixture`] binds none.
pub(crate) const TWO_CYCLES_ONE_SCOPE: &str = "(def g (fn [x y] x)) \
    (def f (fn [k0] \
      (let [k (if (%int? k0) k0 0)] \
        (letrec [a1 (fn [m] (if (%lt m 1) 0 (a2 (%sub m 1)))) \
                 a2 (fn [m] (if (%lt m 1) 0 (a1 (%sub m 1)))) \
                 b1 (fn [m] (if (%lt m 1) 0 (b2 (%sub m 1)))) \
                 b2 (fn [m] (if (%lt m 1) 0 (b1 (%sub m 1))))] \
          (if (%lt k 1) k (g (a1 k) (b1 k)))))))";

/// [`TWO_CYCLES_ONE_SCOPE`] with each cycle bound in its own `letrec`, the
/// second in the tail of the first's body, so one tail call strands an arena of
/// each scope.
pub(crate) const TWO_CYCLES_NESTED: &str = "(def g (fn [x y] x)) \
    (def f (fn [k0] \
      (let [k (if (%int? k0) k0 0)] \
        (letrec [a1 (fn [m] (if (%lt m 1) 0 (a2 (%sub m 1)))) \
                 a2 (fn [m] (if (%lt m 1) 0 (a1 (%sub m 1))))] \
          (letrec [b1 (fn [m] (if (%lt m 1) 0 (b2 (%sub m 1)))) \
                   b2 (fn [m] (if (%lt m 1) 0 (b1 (%sub m 1))))] \
            (if (%lt k 1) k (g (a1 k) (b1 k))))))))";
