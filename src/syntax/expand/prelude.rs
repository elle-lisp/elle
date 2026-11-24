// audited: 2026-11-24
//! Loads the standard prelude: its own compilation unit, whose defmacro forms register the everyday macros.
//!
//! docs/macros.md
//! docs/impl/syntax.md

use super::{Expander, ScopeId, SyntaxArena, UnitTag};
use crate::symbol::SymbolTable;
use crate::vm::VM;

impl Expander {
    /// Load the standard prelude macros.
    ///
    /// Parses and expands `prelude.lisp`, which registers macro
    /// definitions in this Expander. Must be called after the VM
    /// has primitives registered but before user code expansion.
    pub fn load_prelude(&mut self, symbols: &mut SymbolTable, vm: &mut VM) -> Result<(), String> {
        use crate::pipeline::sources::PRELUDE;
        // The prelude is a compilation unit like any other: its tree lives in
        // its own working region, and only the macro templates it registers
        // (copied into the template arena by `handle_defmacro`) outlive it.
        let heap = vm.heap_ptr;
        let work = SyntaxArena::mint(unsafe { &mut *heap });
        let previous = std::mem::replace(&mut self.arena, work);
        let result = self.load_prelude_into(PRELUDE, symbols, vm);
        self.arena = previous;
        unsafe { (*heap).decref_region_if_present(work.region()) };
        result
    }

    fn load_prelude_into(
        &mut self,
        prelude: &str,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<(), String> {
        let syntaxes = crate::reader::read_syntax_all(self.arena, prelude, "<internal>")?;
        self.in_unit(UnitTag::of("<internal>", &syntaxes), |this| {
            // The primitive scope lets a prelude template's symbols resolve to
            // primitives even where the call site binds the same names.
            for syntax in syntaxes {
                let scoped = this.add_scope_recursive(syntax, ScopeId::PRIMITIVE);
                this.expand(scoped, symbols, vm)?;
            }
            Ok(())
        })
    }
}
