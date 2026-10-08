// audited: 2026-10-07
//! A macro definition: its name, its parameter lists, its template, and the
//! cell that caches its compiled transformer.
//!
//! src/syntax/expand/AGENTS.md

use super::Syntax;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;
use std::cell::Cell;
use std::rc::Rc;

/// A macro's parameter lists: the required names, the `&opt` names after
/// them, and the name after `&` that collects the rest.
///
/// The fields are private and each list has a builder step of its own, so a
/// caller cannot put the optional names where the required ones go.
#[derive(Debug, Clone, Default)]
pub struct MacroParams {
    required: Vec<String>,
    optional: Vec<String>,
    rest: Option<String>,
}

impl MacroParams {
    /// Required parameters, and no others.
    pub fn fixed(required: Vec<String>) -> Self {
        MacroParams {
            required,
            ..MacroParams::default()
        }
    }

    /// These parameters, with `optional` after the required ones.
    pub fn with_optional(mut self, optional: Vec<String>) -> Self {
        self.optional = optional;
        self
    }

    /// These parameters, with `rest` collecting every argument after them.
    pub fn with_rest(mut self, rest: Option<String>) -> Self {
        self.rest = rest;
        self
    }

    /// The names a call must supply.
    pub fn required(&self) -> &[String] {
        &self.required
    }

    /// The names a call may supply after the required ones.
    pub fn optional(&self) -> &[String] {
        &self.optional
    }

    /// The name that collects the arguments after the optional ones.
    pub fn rest(&self) -> Option<&str> {
        self.rest.as_deref()
    }
}

/// A macro definition, stored as syntax.
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub params: MacroParams,
    pub template: Syntax,
    transformer: TransformerCell,
}

impl MacroDef {
    /// A definition whose transformer is not compiled yet.
    pub fn new(name: impl Into<String>, params: MacroParams, template: Syntax) -> Self {
        MacroDef {
            name: name.into(),
            params,
            template,
            transformer: TransformerCell::default(),
        }
    }

    /// The cell that caches this definition's compiled transformer. Every
    /// clone of the definition shares it.
    pub(crate) fn transformer(&self) -> &TransformerCell {
        &self.transformer
    }
}

/// The compiled `(fn (params...) template)` closure of one macro definition,
/// filled on first expansion.
///
/// The cell is filled lazily, because a template literal's scopes are the
/// ones its real expansion context gives it. It is shared by every clone of
/// its definition, so the first compile that expands the macro fills it once
/// and every later compile reuses it.
///
/// A filled cell owns one reference to the transformer's region, and the last
/// clone to drop gives it back on the heap the transformer was compiled on.
#[derive(Clone, Default)]
pub(crate) struct TransformerCell(Rc<Slot>);

impl TransformerCell {
    /// The compiled transformer, if an expansion has compiled it.
    pub(crate) fn get(&self) -> Option<Value> {
        self.0.filled.get().map(|f| f.value)
    }

    /// Store a compiled transformer, and take over one reference to its region.
    ///
    /// # Safety
    /// `value` must live on `heap`, and `heap` must outlive every clone of this
    /// cell that still holds it.
    pub(crate) unsafe fn fill(&self, heap: *mut FiberHeap, value: Value) {
        debug_assert!(self.get().is_none(), "a transformer cell is filled once");
        self.0.release();
        self.0.filled.set(Some(Filled { value, heap }));
    }

    /// Give back the region reference, and leave the cell empty for every
    /// clone that shares it.
    pub(crate) fn release(&self) {
        self.0.release();
    }
}

/// What a filled cell holds: the closure, and the heap whose region it is in.
#[derive(Clone, Copy)]
struct Filled {
    value: Value,
    heap: *mut FiberHeap,
}

/// The storage every clone of one cell shares.
#[derive(Default)]
struct Slot {
    filled: Cell<Option<Filled>>,
}

impl Slot {
    fn release(&self) {
        if let Some(Filled { value, heap }) = self.filled.take() {
            // Safe: `fill`'s contract keeps the heap alive while a clone of
            // the cell holds a transformer on it.
            let heap = unsafe { &mut *heap };
            let region = crate::value::arena::region_of(heap, value);
            crate::value::arena::decref_region(heap, region);
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.release();
    }
}

impl std::fmt::Debug for TransformerCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TransformerCell").field(&self.get()).finish()
    }
}
