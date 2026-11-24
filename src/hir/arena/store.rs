// audited: 2026-11-24
//! The arena that owns a compilation unit's bindings, indexed by `Binding`.
//!
//! docs/impl/hir.md

use super::{BindingInner, BindingScope};
use crate::hir::binding::Binding;
use crate::value::SymbolId;

/// The name of the temp a `match` or a `destructure` binds the value it reads
/// out of a 1-slot container to (docs/impl/region/reads.md).
pub const SCRUTINEE: &str = "__scrutinee";

/// The name of the temp a call binds the callee it reads out of a 1-slot
/// container to (docs/impl/region/reads.md).
pub const CALLEE: &str = "__callee";

/// Every name a compiler pass gives a temp. The build fixes each spelling, so
/// a reader that shows a temp's name learns it from this list
/// (docs/spec/form.md).
pub const TEMP_NAMES: &[&str] = &[SCRUTINEE, CALLEE];

/// Arena for compile-time bindings.
///
/// Bindings are allocated during analysis (`&mut self`) and read during
/// lowering (`&self`). The arena is dropped at the end of the compilation
/// unit — no leaks.
///
/// A `Binding(u32)` index is only valid for the arena that created it.
#[derive(Debug)]
pub struct BindingArena {
    bindings: Vec<BindingInner>,
}

impl BindingArena {
    /// Create an empty arena.
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Allocate a new binding. Analysis phase only.
    pub fn alloc(&mut self, name: SymbolId, scope: BindingScope) -> Binding {
        self.alloc_from(BindingInner::new(name, scope))
    }

    /// Allocate a binding from ready-made metadata. The graft of an
    /// [`HirFragment`](crate::hir::fragment::HirFragment) enters here: the
    /// fragment holds a whole `BindingInner` per binding its body introduces,
    /// so re-hosting reproduces the binding rather than describing it again.
    pub fn alloc_from(&mut self, inner: BindingInner) -> Binding {
        let index = self.bindings.len() as u32;
        self.bindings.push(inner);
        Binding(index)
    }

    /// Allocate a synthetic binding with no source-level identity.
    /// Used by compiler passes that need temporaries (for example, phi-insertion
    /// condition bindings). The name is set to `SymbolId::SYNTHETIC`.
    pub fn gensym(&mut self) -> Binding {
        self.synthetic(SymbolId::SYNTHETIC)
    }

    /// Allocate a synthetic local named `name`, one of [`TEMP_NAMES`]: a
    /// binding a compiler pass makes, which no source text names.
    pub fn temp(&mut self, name: &'static str) -> Binding {
        debug_assert!(
            TEMP_NAMES.contains(&name),
            "the temp name {name} is not in TEMP_NAMES"
        );
        self.synthetic(SymbolId::of(name))
    }

    /// Whether `binding` is the temp a call binds its container-reading callee
    /// to ([`CALLEE`]).
    pub fn is_callee_temp(&self, binding: Binding) -> bool {
        let inner = self.get(binding);
        inner.is_synthetic && inner.name == SymbolId::of(CALLEE)
    }

    fn synthetic(&mut self, name: SymbolId) -> Binding {
        let b = self.alloc(name, BindingScope::Local);
        self.get_mut(b).is_synthetic = true;
        b
    }

    /// Read-only access. Available in both analysis and lowering phases.
    pub fn get(&self, binding: Binding) -> &BindingInner {
        &self.bindings[binding.0 as usize]
    }

    /// Mutable access. Analysis phase only (requires `&mut self`).
    pub fn get_mut(&mut self, binding: Binding) -> &mut BindingInner {
        &mut self.bindings[binding.0 as usize]
    }

    /// Number of bindings in the arena.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns true if the arena contains no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

impl Default for BindingArena {
    fn default() -> Self {
        Self::new()
    }
}
