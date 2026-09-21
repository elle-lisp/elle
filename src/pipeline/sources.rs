// audited: 2026-09-21
//! The three Elle sources a boot compiles, embedded at build time.
//!
//! docs/impl/image/boot.md
//!
//! One home for the three, because a boot image is only valid for the sources
//! it was built from and the digest that decides so has to see all three at
//! once.

/// core.lisp: the first Elle this process runs.
pub const CORE: &str = include_str!("../core.lisp");

/// prelude.lisp: the macros every later unit expands through.
pub const PRELUDE: &str = include_str!("../prelude.lisp");

/// stdlib.lisp: the standard library.
pub const STDLIB: &str = include_str!("../stdlib.lisp");
