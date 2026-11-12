// audited: 2026-11-11
//! The arity checks a call runs before its callee, where the compiler did not prove the argument count.
//!
//! docs/impl/vm.md

use super::core::VM;
use crate::value::SIG_ERROR;

impl VM {
    /// Check a closure's arity, and set the arity error on the fiber on a
    /// mismatch. Answers whether the count fits.
    pub(crate) fn check_arity(&mut self, arity: &crate::value::Arity, arg_count: usize) -> bool {
        if arity.matches(arg_count) {
            return true;
        }

        let msg = match arity {
            crate::value::Arity::Exact(n) => {
                format!("expected {} arguments, got {}", n, arg_count)
            }
            crate::value::Arity::AtLeast(n) => {
                format!("expected at least {} arguments, got {}", n, arg_count)
            }
            crate::value::Arity::Range(min, max) => {
                format!("expected {}-{} arguments, got {}", min, max, arg_count)
            }
        };
        let err = self.escaping_error("arity-error", msg);
        self.fiber.signal = Some((SIG_ERROR, err));
        false
    }

    /// Check the native `def`'s arity, and set the arity error on the fiber on a
    /// mismatch, naming the native. Answers whether the count fits. The
    /// interpreter runs it at a call whose count the compiler did not check.
    pub(crate) fn check_native_arity(
        &mut self,
        def: &crate::primitives::def::PrimitiveDef,
        arg_count: usize,
    ) -> bool {
        if def.arity.matches(arg_count) {
            return true;
        }
        self.set_error(
            "arity-error",
            format!(
                "{}: expected {} argument(s), got {}",
                def.name, def.arity, arg_count
            ),
        );
        false
    }
}
