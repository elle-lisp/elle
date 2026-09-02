use super::*;

/// Create a dummy root closure for the root fiber.
/// The root fiber doesn't execute a closure directly — it's the
/// execution context for top-level bytecode. This closure is never
/// called; it exists only to satisfy Fiber's constructor.
fn root_closure() -> Rc<Closure> {
    use crate::value::types::Arity;
    use crate::value::ClosureTemplate;
    Rc::new(Closure {
        template: crate::value::TemplateRef::new(Rc::new(ClosureTemplate::new(
            Rc::new(vec![]),
            Arity::Exact(0),
            Rc::new(vec![]),
        ))),
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    })
}

impl VM {
    /// Build a standalone VM that owns a private, leaked `FiberHeap`. Used by test
    /// scaffolding and any VM with no owning `RuntimeCore` (a standalone
    /// macro-expansion VM). Each call leaks its own heap, so two such VMs on one
    /// thread get *distinct* heaps — values one builds are invisible to the other
    /// (coexistence). The leak is deliberate: values built on it stay valid for the
    /// process even after the VM drops (the VM does not own the heap). An instance
    /// that shares its heap with a sibling VM is built through
    /// [`VM::new_with_heap`] by its `RuntimeCore`.
    pub fn new() -> Self {
        let heap_ptr: *mut crate::value::fiberheap::FiberHeap =
            Box::leak(Box::new(crate::value::fiberheap::FiberHeap::new()));
        Self::on_heap(heap_ptr)
    }

    /// The Unicode segmentation generation this VM was constructed with.
    pub fn unicode_generation(&self) -> crate::segment::Generation {
        self.unicode_generation
    }

    /// Select the generation. Construction-time only (RuntimeCore,
    /// worker spawn, embedding): no string may yet have been segmented
    /// on this VM, or cluster-derived state (text-port leftovers,
    /// indices) would silently change meaning.
    pub(crate) fn set_unicode_generation(&mut self, gen: crate::segment::Generation) {
        self.unicode_generation = gen;
    }

    /// Where this VM's instance caches its compiled stdlib — what a `sys/spawn`
    /// worker inherits.
    pub fn stdlib_cache(&self) -> &crate::compiler::stdlib_cache::StdlibCache {
        &self.stdlib_cache
    }

    /// Record it. Construction-time only, from `RuntimeCore::load_stdlib`.
    pub(crate) fn set_stdlib_cache(&mut self, cache: crate::compiler::stdlib_cache::StdlibCache) {
        self.stdlib_cache = cache;
    }

    /// Build a VM pointing at an externally-owned heap (`RuntimeCore`'s
    /// `Box<FiberHeap>`), the coexistence-correct constructor: the program VM and
    /// its instance's macro-expansion VM share this one heap, so core.lisp and
    /// stdlib closures (created on the macro/program VM) and the values user code
    /// builds at runtime all live in the same region store. Two embedded
    /// instances on one thread each own a distinct heap and pass it here, so
    /// neither sees the other's regions (tls.md § Acceptance criterion).
    pub fn new_with_heap(heap_ptr: *mut crate::value::fiberheap::FiberHeap) -> Self {
        Self::on_heap(heap_ptr)
    }

    /// Shared VM construction over an explicit heap pointer: build this instance's
    /// default trait tables on it (idempotent) and assemble the VM with `heap_ptr`
    /// set. Every allocation reaches the heap through `heap_ptr`.
    fn on_heap(heap_ptr: *mut crate::value::fiberheap::FiberHeap) -> Self {
        // Initialize this instance's default trait tables for collection/sequence
        // types. They allocate root objects into the heap's own pinned root
        // region and the per-tag table lives on the heap itself (instance state,
        // not thread state); held alive by RC and released by the teardown sweep.
        // Idempotent, so the program and macro VM sharing one heap build it once.
        crate::primitives::traitregistry::init_default_traits(unsafe { &mut *heap_ptr });

        let mut fiber = Fiber::new(root_closure(), SIG_OK);
        // Root fiber starts alive (it's the currently executing context)
        fiber.status = crate::value::FiberStatus::Alive;

        // Root the VM's trace state in THIS instance's heap cell (a clone), so
        // `runtime_config.has_trace_bit` and the off-VM readers (region pages,
        // channels) all read one per-instance bitfield. `from_static_config`
        // seeds it from the CLI `--trace=` keywords and mirrors the POSIX bit onto
        // the constructing thread (the scheduler thread).
        let trace_cell = unsafe { &*heap_ptr }.trace_cell();
        let rc = crate::config::RuntimeConfig::from_static_config(crate::config::get(), trace_cell);

        #[cfg(feature = "mlir")]
        let mlir_enabled = rc.mlir.enabled();

        VM {
            runtime_config: rc,
            unicode_generation: crate::config::get().unicode_generation(),
            stdlib_cache: crate::compiler::stdlib_cache::StdlibCache::default(),
            heap_ptr,
            compile_ctx_ptr: std::ptr::null_mut(),
            symbols_ptr: std::ptr::null_mut(),
            fiber,
            current_fiber_handle: None, // root fiber has no handle
            current_fiber_value: None,  // root fiber has no Value
            ffi: FFISubsystem::new(),
            loading_modules: std::collections::HashSet::new(),
            loaded_plugins: HashMap::new(),
            closure_call_counts: FxHashMap::default(),
            tail_call_env_cache: Vec::with_capacity(256),
            env_cache: Vec::with_capacity(256),
            pending_tail_call: None,
            pending_fiber_resume: None,
            pending_entry_closure: crate::value::Value::NIL,
            pending_error_park: false,
            trampoline_parent_override: None,
            error_loc: None,
            gated_exit_reason: None,
            active_tier: "bytecode",
            exit_trapped: false,
            #[cfg(feature = "jit")]
            jit_cache: FxHashMap::default(),
            #[cfg(feature = "jit")]
            jit_worker: None,
            #[cfg(feature = "jit")]
            jit_pending: FxHashMap::default(),
            #[cfg(feature = "jit")]
            jit_rejections: FxHashMap::default(),
            #[cfg(feature = "jit")]
            jit_compile_attempts: FxHashMap::default(),
            docs: HashMap::new(),
            eval_expander: None,
            user_args: Vec::new(),
            source_arg: String::new(),
            #[cfg(feature = "wasm")]
            wasm_tier: if crate::config::get().wasm_tier_enabled() {
                crate::wasm::lazy::WasmTier::new().ok()
            } else {
                None
            },
            #[cfg(feature = "wasm")]
            wasm_rejections: FxHashMap::default(),
            #[cfg(feature = "mlir")]
            mlir_enabled,
            #[cfg(feature = "mlir")]
            mlir_cache: None,
        }
    }

    /// Reset the VM's fiber and transient state for reuse.
    ///
    /// Preserves: docs, ffi, jit_cache, eval_expander, env_cache,
    /// tail_call_env_cache, fiber heap Box (reused for pointer stability).
    /// Resets: fiber, call state, location map,
    /// loaded modules, closure call counts.
    pub fn reset_fiber(&mut self) {
        // The VM heap is persistent — don't clear it. Values from previous
        // execute_bytecode calls remain valid.
        self.fiber = Fiber::new(root_closure(), SIG_OK);
        self.fiber.status = crate::value::FiberStatus::Alive;
        self.current_fiber_handle = None;
        self.current_fiber_value = None;
        self.pending_tail_call = None;
        self.pending_entry_closure = crate::value::Value::NIL;
        self.pending_error_park = false;
        self.pending_fiber_resume = None;
        self.error_loc = None;
        self.active_tier = "bytecode";
        self.closure_call_counts.clear();
        #[cfg(feature = "jit")]
        self.jit_rejections.clear();
        #[cfg(feature = "jit")]
        self.jit_compile_attempts.clear();
        self.loading_modules.clear();
    }
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}
