// audited: 2026-09-09
//! The ambient environment a compilation starts from: the primitives, and what
//! `begin-for-syntax` or the core library exports.
//!
//! docs/impl/hir.md
//!
//! Both bind a name to a compile-time constant VALUE, which is what the lowerer
//! emits as `LoadConst` instead of a global lookup. Both therefore record from the
//! value, never from the name: the core env's whole purpose is to bind a bytecode
//! closure over a name a native also answers to.

use super::{Analyzer, Binding, BindingScope, HashMap, PrimitiveMeta, Value};

impl Analyzer<'_> {
    /// Bind all registered primitives as immutable Local bindings in the
    /// analyzer's initial scope.
    ///
    /// Called before `analyze_file_letrec` so that primitives are in scope
    /// during file analysis. Primitives are `BindingScope::Local` with
    /// `mark_immutable()` set. File-level `def` bindings shadow primitives
    /// because `analyze_file_letrec` pushes a new scope.
    ///
    /// The lowerer uses `immutable_values` to emit `LoadConst` for these
    /// bindings — the `NativeFn` values are baked into the constant pool.
    /// No slot allocation is needed.
    pub fn bind_primitives(&mut self, meta: &PrimitiveMeta) {
        for (&sym_id, &signal) in &meta.signals {
            let binding = self.bind_by_sym(sym_id, BindingScope::Local);
            self.arena.get_mut(binding).is_immutable = true;
            self.arena.get_mut(binding).is_primitive = true;
            self.signal_env.insert(binding, signal);
            if let Some(&arity) = meta.arities.get(&sym_id) {
                self.arity_env.insert(binding, arity);
            }
            if let Some(&func_value) = meta.functions.get(&sym_id) {
                self.primitive_values.insert(binding, func_value);
                self.arena.get_mut(binding).is_native_fn = func_value.is_native_fn();
            }
        }
    }

    /// Return the primitive binding→value map for the lowerer.
    ///
    /// The lowerer seeds its `immutable_values` from this so that
    /// primitive references compile to `LoadConst`.
    pub fn primitive_values(&self) -> &HashMap<Binding, Value> {
        &self.primitive_values
    }

    /// Bind compile-time values (from `begin-for-syntax`) into the Analyzer's
    /// current scope as immutable local bindings backed by constant values.
    ///
    /// Called from `eval_syntax` after `bind_primitives` so that compile-time
    /// names are visible in macro body analysis. The Lowerer emits `LoadConst`
    /// for these bindings (same mechanism as primitive functions).
    ///
    /// `env`: map from name string to Value, from `Expander.compile_time_env`.
    ///
    /// `is_primitive`: mark each binding as a primitive. This is for the
    /// **core.lisp export env** (`fold`/`reduce`/`concat`/`append`/`reverse`/…),
    /// whose bindings here are the *canonical* definitions user code calls — they
    /// deliberately override any earlier `meta` entry (e.g. `reverse` was once a
    /// native, now core.lisp), so this binding must win *and* be recognizable to
    /// passes that key on `is_primitive` (loop fusion, dispatch monomorphization),
    /// exactly as stdlib exports are. Without the flag a core HOF is invisible to
    /// those passes even though it is as canonical as `map`/`filter`. Passed false
    /// for genuine compile-time / REPL envs, where a binding is a user value that
    /// must *not* be treated as a canonical primitive.
    pub fn bind_compile_time_env(
        &mut self,
        env: &std::collections::HashMap<String, crate::value::Value>,
        is_primitive: bool,
    ) {
        for (name, value) in env {
            let sym = self.symbols.intern(name);
            let binding = self.bind_by_sym(sym, BindingScope::Local);
            self.arena.get_mut(binding).is_immutable = true;
            self.arena.get_mut(binding).is_primitive = is_primitive;
            // From the VALUE, not from `is_primitive`: the core env's whole point is
            // to bind a bytecode CLOSURE over a name a native also answers to, and a
            // call to that name replaces the frame (`may_replace_frame`).
            self.arena.get_mut(binding).is_native_fn = value.is_native_fn();
            self.primitive_values.insert(binding, *value);
        }
    }
}
