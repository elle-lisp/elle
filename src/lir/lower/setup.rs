// audited: 2026-09-21
//! Building a `Lowerer`: the constructor and the `with_*` builders that hand
//! it the front end's analysis products before `lower` runs.
//!
//! docs/impl/lir.md

use super::*;

impl<'a> Lowerer<'a> {
    pub fn new(arena: &'a BindingArena) -> Self {
        Lowerer {
            arena,
            symbols: None,
            current_func: LirFunction::new(Arity::Exact(0)),
            current_block: BasicBlock::new(Label(0)),
            next_reg: 0,
            next_label: 1, // 0 is entry
            binding_to_slot: HashMap::new(),
            in_lambda: false,
            num_captures: 0,
            num_local_params: 0,
            upvalue_bindings: std::collections::HashSet::new(),
            compiled_cell_bindings: std::collections::HashSet::new(),
            destructure_alias_bindings: std::collections::HashSet::new(),
            current_span: Span::synthetic(),
            intrinsics: FxHashMap::default(),
            call_classification: crate::hir::CallClassification::default(),
            immutable_values: HashMap::new(),
            loop_lower_contexts: Vec::new(),
            block_lower_contexts: Vec::new(),
            pending_free_regions: Vec::new(),
            discard_slot: None,
            closures: Vec::new(),
            current_function_binding: None,
            pending_lambda_name: None,
            current_self_binding: None,
            current_function_params: None,
            self_recursive_bindings: rustc_hash::FxHashSet::default(),
            stranded_self_bindings: rustc_hash::FxHashSet::default(),
            stranded_cycle_bindings: rustc_hash::FxHashSet::default(),
            stranded_member_bindings: rustc_hash::FxHashSet::default(),
            region_info: RegionInfo::empty(),
            escape_info: EscapeInfo::empty(),
            increfs_by_site: HashMap::new(),
            decrefs_by_decref_point: HashMap::new(),
            cell_drops_by_demise: HashMap::new(),
            current_hir_id: None,
            region_to_table: HashMap::new(),

            region_to_slot: HashMap::new(),
            reassigned_local_slots: rustc_hash::FxHashSet::default(),
            emitted_alloc_regions: rustc_hash::FxHashSet::default(),
            deferred_decref_points: rustc_hash::FxHashSet::default(),
            return_minted_calls: rustc_hash::FxHashSet::default(),
            tail_exit_hoist: Vec::new(),
            arm_exit_hoists: Vec::new(),
            replicating_release: false,
            hir_types: HashMap::new(),
        }
    }

    /// Give lowering the front end's inferred types, so a proven operation can
    /// carry its operand proof into LIR (docs/impl/lir.md).
    pub fn with_type_info(mut self, info: crate::hir::TypeInfo) -> Self {
        self.hir_types = info.hir_types;
        self
    }

    /// Set all primitive property sets from a PrimitiveClassification.
    pub fn with_primitive_classification(
        mut self,
        pc: crate::lir::intrinsics::PrimitiveClassification,
    ) -> Self {
        self.intrinsics = pc.intrinsics;
        self.call_classification = pc.call_classification;
        self
    }

    /// Seed `immutable_values` with primitive binding→value pairs.
    ///
    /// Primitive bindings are `BindingScope::Local` with `mark_immutable()`.
    /// The lowerer never allocates slots for them — instead, `lower_var`
    /// checks `immutable_values` first and emits `LoadConst` for any
    /// binding with a known constant value.
    pub fn with_primitive_values(mut self, values: HashMap<Binding, Value>) -> Self {
        self.immutable_values.extend(values);
        self
    }

    /// Give lowering the instance's display memo, so an `undefined variable`
    /// error names the variable the user wrote.
    pub fn with_symbols(mut self, symbols: &'a crate::symbol::SymbolTable) -> Self {
        self.symbols = Some(symbols);
        self
    }

    /// The spelling of `binding`'s name, for `pending_lambda_name`. `None`
    /// when lowering runs without a symbol table or the name is a gensym
    /// with no spelling.
    pub(super) fn binder_name(&self, binding: Binding) -> Option<String> {
        let sym = self.arena.get(binding).name;
        self.symbols?.name(sym).map(str::to_string)
    }
}
