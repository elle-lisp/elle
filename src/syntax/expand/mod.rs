// audited: 2026-10-22
//! Hygienic macro expansion: the macro table, the unit under expansion, and the
//! walk that rewrites a form until no macro heads it.
//!
//! docs/macros.md
//! docs/impl/syntax.md
//!
//! Every scope an expander mints names the unit it is expanding, so a compile
//! begins its unit before it expands, and a transformer's compile runs in a
//! unit of its own.

mod collections;
mod compiletime;
mod define;
mod include;
mod introspection;
mod macro_expand;
mod macrodef;
mod quasiquote;
mod scope;
mod syntaxcase;
#[cfg(test)]
mod tests;

pub use macrodef::{MacroDef, MacroParams};

use super::{ScopeId, Span, Syntax, SyntaxArena, SyntaxKind, Unit, UnitTag};
use crate::primitives::def::PrimitiveMeta;
use crate::symbol::SymbolTable;
use crate::vm::VM;
use std::collections::HashMap;
use std::rc::Rc;

/// Maximum macro expansion depth before erroring (prevents infinite expansion)
const MAX_MACRO_EXPANSION_DEPTH: usize = 200;

/// Hygienic macro expander
pub struct Expander {
    /// Where the tree being expanded is born — the working arena of the
    /// compilation unit under expansion. Set per unit; a bare expander starts
    /// on the template arena so a test that never sets one still allocates
    /// somewhere live.
    arena: SyntaxArena,
    /// Where macro templates are copied to when a macro is defined. This is
    /// the instance's process-root syntax arena: a `MacroDef` outlives the
    /// unit that defined it and every unit that expands it
    /// (docs/impl/syntax.md).
    templates: SyntaxArena,
    macros: HashMap<String, MacroDef>,
    /// Compile-time values defined by `begin-for-syntax` blocks.
    /// Always starts empty — the manual `Clone` impl resets it so that
    /// compile-time defs never leak between pipeline calls via the cache.
    pub(crate) compile_time_env: HashMap<String, crate::value::Value>,
    /// Pre-prelude exports from core.lisp. Persists across clones so
    /// that macro bodies compiled via `eval_syntax` can reference
    /// core functions like `last` and `butlast`.
    pub(crate) core_env: HashMap<String, crate::value::Value>,
    /// Primitive (+ stdlib export) metadata used to compile macro-transformer
    /// bodies via `eval_syntax`. `Rc` so that the per-pipeline-call clone is a
    /// pointer bump rather than a deep copy of the maps. The owning instance's
    /// `CompileCtx` sets this to `primitives` at construction and to
    /// `primitives + stdlib exports` once `init_stdlib` runs, and adds each
    /// primitive a host registers. REPL and host bindings are deliberately NOT
    /// included (macro bodies never resolve them).
    /// Because it rides on the `Expander`, `eval_syntax` reaches it without a
    /// separate `CompileCtx` borrow — which would alias the macro VM mid-expand.
    eval_meta: Rc<PrimitiveMeta>,
    /// The unit under expansion, and what it has minted.
    unit: Unit,
    /// The unit's include log, and the chain of files being included.
    includes: crate::pipeline::Includes,
    expansion_depth: usize,
}

impl Clone for Expander {
    fn clone(&self) -> Self {
        Expander {
            arena: self.arena,
            templates: self.templates,
            macros: self.macros.clone(),
            compile_time_env: HashMap::new(), // always fresh — never inherit compile-time defs
            core_env: self.core_env.clone(),  // persists — needed by macro bodies
            eval_meta: Rc::clone(&self.eval_meta), // shared pointer; immutable after stdlib load
            unit: self.unit,
            includes: crate::pipeline::Includes::default(),
            expansion_depth: self.expansion_depth,
        }
    }
}

impl Expander {
    /// Build an expander over `heap`'s process-root syntax arena.
    ///
    /// # Safety
    /// `heap` must point at a live `FiberHeap` for the expander's whole life.
    pub unsafe fn new(heap: *mut crate::value::fiberheap::FiberHeap) -> Self {
        let templates = SyntaxArena::templates(heap);
        Expander {
            arena: templates,
            templates,
            macros: HashMap::new(),
            compile_time_env: HashMap::new(),
            core_env: HashMap::new(),
            eval_meta: Rc::new(PrimitiveMeta::default()),
            // An expander that no compile began a unit on, as a test's is,
            // still mints under a tag of its own.
            unit: Unit::begin(UnitTag::of("<expander>", &[])),
            includes: crate::pipeline::Includes::default(),
            expansion_depth: 0,
        }
    }

    /// Build an expander over `vm`'s heap — the safe form of [`new`](Self::new)
    /// for a caller that already holds the VM its transformers will run on.
    pub fn on_vm(vm: &mut VM) -> Self {
        // Safe: the heap the VM points at is leaked for the process (or owned
        // by the instance that built the VM), so it outlives the expander.
        unsafe { Expander::new(vm.heap_ptr) }
    }

    /// The arena the tree under expansion is born in.
    pub fn arena(&self) -> SyntaxArena {
        self.arena
    }

    /// Point this expander at the working arena of the unit it is about to
    /// expand: the arena's region dies with the unit, so every node the
    /// expansion builds dies with it too.
    ///
    /// The pipeline does not call this directly. `CompileCtx`'s
    /// `with_macro_expansion` and `expander_and_meta` take the arena and hand
    /// out an expander already pointed at it. So an expansion cannot end up
    /// allocating into the process-root template arena by omission. It is here
    /// for the two expanders the pipeline does not own: the VM's cached `eval`
    /// expander and the bare one that loads core.lisp.
    pub fn set_arena(&mut self, arena: SyntaxArena) {
        self.arena = arena;
    }

    /// The primitive(+stdlib) metadata for compiling macro-transformer bodies
    /// (`eval_syntax`). See the `eval_meta` field.
    pub fn eval_meta(&self) -> &PrimitiveMeta {
        &self.eval_meta
    }

    /// Replace the macro-body compile metadata. The owning `CompileCtx` calls
    /// this with `primitives` at construction and `primitives + stdlib exports`
    /// after `init_stdlib`, and `register_primitive` calls it with each
    /// primitive a host registers.
    pub fn set_eval_meta(&mut self, meta: PrimitiveMeta) {
        self.eval_meta = Rc::new(meta);
    }

    /// Register a macro definition
    pub fn define_macro(&mut self, def: MacroDef) {
        self.macros.insert(def.name.clone(), def);
    }

    /// Check if any macros are registered (used to detect if prelude is loaded)
    pub fn has_macros(&self) -> bool {
        !self.macros.is_empty()
    }

    /// Return the macro definitions. The REPL reads them to keep the macros a
    /// line defined for later lines (`CompileCtx::register_repl_macros`).
    pub fn macros(&self) -> &HashMap<String, MacroDef> {
        &self.macros
    }

    /// Start expanding the unit `tag` names: every scope this expander mints
    /// from here names that unit, its counts start at zero, and the
    /// `begin-for-syntax` definitions and the include log of the last unit are
    /// gone.
    ///
    /// The pipeline does not call this directly — `CompileCtx`'s
    /// `with_macro_expansion` and `expander_and_meta` take the unit and hand
    /// out an expander that has begun it, as they do the arena. It is here for
    /// the VM's cached `eval` expander and the bare one that loads core.lisp.
    pub(crate) fn begin_unit(&mut self, tag: UnitTag) {
        self.unit = Unit::begin(tag);
        self.compile_time_env.clear();
        self.includes = crate::pipeline::Includes::default();
    }

    /// Run `f` in the unit `tag` names, and resume this expander's unit after
    /// it, with the counts it had. A transformer's compile and the prelude run
    /// this way, inside whatever unit first needs them.
    pub(crate) fn in_unit<R>(&mut self, tag: UnitTag, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = std::mem::replace(&mut self.unit, Unit::begin(tag));
        let out = f(self);
        self.unit = outer;
        out
    }

    /// The unit under expansion, for the VM to borrow while code the
    /// expansion runs calls `gensym`.
    pub(crate) fn unit_mut(&mut self) -> &mut Unit {
        &mut self.unit
    }

    /// Merge macro definitions from another Expander. Existing macros
    /// with the same name are overwritten. A REPL line's expander takes the
    /// macros earlier lines defined this way.
    pub fn merge_macros(&mut self, other: &HashMap<String, MacroDef>) {
        for (name, def) in other {
            self.macros.insert(name.clone(), def.clone());
        }
    }

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

    /// Release the region reference each cached transformer holds, and leave
    /// every cell empty. A cell releases on its own when its last clone drops,
    /// but the instance's master expander drops after teardown has counted the
    /// residue. `CompileCtx::release` calls this first, so the count sees none.
    pub fn release_cached_transformers(&mut self) {
        for def in self.macros.values() {
            def.transformer().release();
        }
    }

    /// Create a symbol syntax node
    fn make_symbol(&self, name: &str, span: Span) -> Syntax {
        Syntax::symbol(&self.arena, name, span)
    }

    /// Create a list syntax node
    fn make_list(&self, items: Vec<Syntax>, span: Span) -> Syntax {
        Syntax::list(&self.arena, &items, span)
    }

    /// Expand every macro in `syntax`, and push what it expands to onto `out`:
    /// one form, or for an include, each form of the file it names.
    fn expand_into(
        &mut self,
        syntax: Syntax,
        out: &mut Vec<Syntax>,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<(), String> {
        // Point the transformer-running VM at the table THIS expansion uses, so a
        // transformer's `gensym`/`syntax->datum`/`read` (via `ctx.vm().symbols()`)
        // interns into the same table the expander threads. Idempotent across
        // recursion; the table is reborrowed per transformer call, never touched
        // simultaneously (docs/impl/region/ctx.md).
        vm.set_symbols(symbols as *mut SymbolTable);
        // Keyword spellings must be learned BEFORE expansion: a macro argument
        // or quasiquote template round-trips a keyword as a bare hash, and the
        // value → syntax conversion on the way back needs the spelling.
        crate::syntax::convert::learn_keywords(&syntax, symbols);
        let expanded = match &syntax.kind {
            SyntaxKind::Symbol(_) => Ok(syntax),
            SyntaxKind::List(items) if !items.is_empty() => match items[0].as_symbol() {
                // A definition registers and expands to nil.
                Some("defmacro" | "define-macro") => self.handle_defmacro(items, &syntax.span),
                Some("macro?") => self.handle_macro_predicate(items, &syntax.span),
                Some("expand-macro") => self.handle_expand_macro(items, &syntax.span, symbols, vm),
                Some("begin-for-syntax") => {
                    self.handle_begin_for_syntax(items, &syntax.span, symbols, vm)
                }
                Some("syntax-case") => self.handle_syntax_case(items, &syntax.span, symbols, vm),
                Some(name) => {
                    if let Some(include) = crate::pipeline::IncludeName::of(&syntax) {
                        return self.splice_include(syntax, include, out, symbols, vm);
                    }
                    if let Some(macro_def) = self.macros.get(name).cloned() {
                        let args = &items[1..];
                        return self.expand_macro_call(&macro_def, args, &syntax, out, symbols, vm);
                    }
                    self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::List)
                }
                None => self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::List),
            },
            SyntaxKind::Array(items) => {
                self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::Array)
            }
            SyntaxKind::ArrayMut(items) => {
                self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::ArrayMut)
            }
            SyntaxKind::Struct(items) => {
                self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::Struct)
            }
            SyntaxKind::StructMut(items) => {
                self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::StructMut)
            }
            SyntaxKind::Set(items) => self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::Set),
            SyntaxKind::SetMut(items) => {
                self.expand_seq(items, &syntax, symbols, vm, SyntaxKind::SetMut)
            }
            SyntaxKind::Quote(_) => {
                // Don't expand inside quote
                Ok(syntax)
            }
            SyntaxKind::Quasiquote(inner) => {
                // Convert quasiquote to code that builds the structure
                self.quasiquote_to_code(inner, 1, &syntax.span, symbols, vm)
            }
            SyntaxKind::Splice(inner) => {
                let expanded = self.expand(**inner, symbols, vm)?;
                Ok(Syntax::with_scope_slice(
                    SyntaxKind::Splice(self.arena.node(expanded)),
                    syntax.span,
                    syntax.scope_slice(),
                ))
            }
            SyntaxKind::StringMut(s) => {
                // @"..." desugars to (%thaw "...") at expansion time — the
                // intrinsic, not the `thaw` wrapper, so inference proves the
                // result is a mutable string (the wrapper is polymorphic and
                // types Top). `%thaw` is Total (never rejects) and lowers to the
                // same funnel native as the wrapper, so the runtime is identical.
                let thaw_sym =
                    Syntax::symbol_scoped(&self.arena, "%thaw", syntax.span, &[ScopeId::PRIMITIVE]);
                let str_lit = Syntax::with_scope_slice(
                    SyntaxKind::String(*s),
                    syntax.span,
                    syntax.scope_slice(),
                );
                Ok(Syntax::with_scope_slice(
                    SyntaxKind::List(self.arena.nodes(&[thaw_sym, str_lit])),
                    syntax.span,
                    syntax.scope_slice(),
                ))
            }
            _ => Ok(syntax),
        };
        out.push(expanded?);
        Ok(())
    }
}
