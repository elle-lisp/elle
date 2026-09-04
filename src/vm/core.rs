use crate::error::StackFrame;
use crate::ffi::FFISubsystem;
use crate::hir::region::{MappedRegion, RuntimeRegion, StaticRegion};
use crate::primitives::def::Doc;
use crate::reader::SourceLoc;
use crate::value::{
    BytecodeFrame, Closure, Fiber, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR,
    SIG_FUEL, SIG_HALT, SIG_OK, SIG_SWITCH,
};
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::rc::Rc;
// `jit_cache` is the only `Arc` holder in this module.
#[cfg(feature = "jit")]
use std::sync::Arc;

#[cfg(feature = "jit")]
use crate::jit::{JitCode, JitRejectionInfo};

/// A `jit_cache` entry: the compiled code plus the pin that keeps the keyed
/// bytecode alive (docs/impl/jit.md § "Cache identity"). The pin makes the
/// raw-address key sound: bytecode lives in a code object's payload, and the
/// pinned code object holds that payload's region, so the address cannot be
/// reused by a different function while this entry lives.
#[cfg(feature = "jit")]
pub struct JitCacheEntry {
    _pin: crate::value::ClosureTemplate,
    pub code: Arc<JitCode>,
}

#[cfg(feature = "jit")]
impl JitCacheEntry {
    /// Build an entry pinning `template` — the code object the entry's cache
    /// key was derived from.
    pub fn new(template: crate::value::ClosureTemplate, code: Arc<JitCode>) -> Self {
        JitCacheEntry {
            _pin: template,
            code,
        }
    }
}

pub(crate) struct TailCallInfo {
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// The callee CLOSURE this tail call re-enters the activation as. A
    /// frame-replacing tail call keeps the same Rust/activation frame but swaps
    /// which closure is executing (a self-recursive `loop` re-enters as itself; a
    /// tail call to a sibling re-enters as the sibling). `trampoline_loop` installs
    /// it as `fiber.current_closure` on the replacement so the executing-closure
    /// register tracks the frame across TCO. `NIL` for a callee with no closure
    /// value (a native/parameter tail target sets no pending tail call, so this is
    /// always a real closure in practice).
    pub closure: Value,
    pub squelch_mask: SignalBits,
}

/// Pending fiber resume for the trampoline.
///
/// Set by `handle_fiber_resume_signal` when it wants to switch fibers
/// without recursing. Consumed by the trampoline in `do_fiber_resume`.
pub(crate) struct PendingFiberResume {
    pub handle: FiberHandle,
    pub fiber_value: Value,
    /// The TRUE parent of the pending child — the fiber whose code called
    /// `fiber/resume`. The trampoline descends from the ROOT context (the
    /// requesting fiber is swapped out by then), so `with_child_fiber`'s
    /// default parent wiring (the currently active fiber) would record the
    /// wrong parent. Carried here and installed via the VM's
    /// `trampoline_parent_override` for the descent.
    pub parent: Option<(FiberHandle, Option<Value>)>,
}

pub struct VM {
    /// Mutable runtime configuration: trace flags, JIT/WASM policy.
    /// Accessible from Elle via `(vm/config)`.
    pub runtime_config: crate::config::RuntimeConfig,
    /// The Unicode segmentation generation for every grapheme operation
    /// this VM performs. Fixed at construction (readable via
    /// `(vm/config :unicode)`, never settable): text ports stash bytes
    /// split at cluster boundaries, so a mid-run change would corrupt
    /// their framing.
    pub(crate) unicode_generation: crate::segment::Generation,
    /// Where this instance caches its compiled stdlib. Recorded by
    /// `RuntimeCore::load_stdlib`, and read by `sys/spawn`: a worker builds its
    /// own runtime on a new thread and reaches the spawning instance only
    /// through `ctx.vm()`, so this is how it inherits the directory its parent
    /// was given instead of falling back to the process-wide one.
    pub(crate) stdlib_cache: crate::compiler::stdlib_cache::StdlibCache,
    /// Pointer to this instance's heap. The VM does not own it: a `RuntimeCore`
    /// owns it as a sibling `Box<FiberHeap>` (`VM::new_with_heap`), or — for a bare
    /// VM with no `RuntimeCore` — it is a privately leaked heap (`VM::new`). Either
    /// way it outlives the VM, and is private to this instance: two coexisting
    /// instances on one thread never share it. Access via `self.heap()` or directly
    /// for split borrows.
    pub(crate) heap_ptr: *mut crate::value::fiberheap::FiberHeap,
    /// Pointer to the owning instance's compile context, set by `RuntimeCore`.
    /// The VM does not own it — its `RuntimeCore` does, as a sibling. The
    /// runtime `eval` instruction reaches the instance's macro expander, core
    /// env, and stdlib metadata through it. Null for a bare VM with no
    /// `RuntimeCore` (a macro-expansion VM, or a test VM that never runs
    /// `(eval …)`).
    pub(crate) compile_ctx_ptr: *mut crate::pipeline::CompileCtx,
    /// Pointer to the owning instance's symbol table, set by `RuntimeCore`.
    /// The VM does not own it — its `RuntimeCore` does, as a sibling whose boxed
    /// `SymbolTable` has a stable address. The runtime `eval` instruction, the
    /// `meta`/`read`/`debug` primitives, and value name-resolution reach the
    /// instance's table through `vm.symbols()` / `ctx.vm().symbols()`, so two
    /// embedded instances on one thread each resolve names in their OWN table.
    /// Null for a bare VM with no `RuntimeCore` (a macro-expansion VM, or a test
    /// VM that never resolves a symbol name); the readers treat null as "table
    /// unavailable" and error or skip name resolution.
    pub(crate) symbols_ptr: *mut crate::symbol::SymbolTable,
    /// The current fiber holding all per-execution state:
    /// operand stack, call frames, exception handlers, fiber state.
    pub fiber: Fiber,
    /// Handle to the current fiber's FiberHandle, if it came from a
    /// `fiber/new` allocation. `None` for the root fiber (which lives
    /// directly on the VM, not behind a handle). Used to wire up
    /// `child.parent` back-pointers during fiber resume.
    pub current_fiber_handle: Option<FiberHandle>,
    /// Cached Value for the current fiber. `None` for the root
    /// fiber. Used to set `child.parent_value` during resume chain wiring,
    /// so `fiber/parent` can return the original Value without re-allocating.
    pub current_fiber_value: Option<Value>,
    pub(crate) ffi: FFISubsystem,
    /// Modules currently being loaded (circular-import guard).
    /// Added before execution, removed after. If a module is in this set
    /// when import-file is called, it's a circular dependency.
    pub loading_modules: std::collections::HashSet<String>,
    /// Plugins already loaded (path → return value). Prevents double-loading
    /// which would re-register primitives and leak library handles.
    pub loaded_plugins: HashMap<String, Value>,
    pub closure_call_counts: FxHashMap<*const u8, usize>,
    pub tail_call_env_cache: Vec<Value>,
    pub env_cache: Vec<Value>,
    pub(crate) pending_tail_call: Option<TailCallInfo>,
    pub(crate) pending_fiber_resume: Option<PendingFiberResume>,
    /// One-shot "the closure whose body is about to run", set immediately before
    /// entering a body via `execute_bytecode_saving_stack` or the raw
    /// `execute_proto`, which take it (resetting to `NIL`) and install it as
    /// `fiber.current_closure` for that activation. **Every entrant that runs a
    /// closure body must set it** — the interpreter call path, the JIT helpers'
    /// interpreter fallback and tail-call resolution, the forced-tier entries,
    /// the fiber's first resume, the measured-thunk entry, the macro-transformer
    /// call, the FFI callback trampoline, the WASM host's bytecode fallback, and
    /// the spawned-worker body — or the body's `LoadSelf` resolves a
    /// self-reference to `NIL` (docs/impl/vm.md § The executing-closure
    /// register; `handle_load_self` debug-asserts the register is populated). A
    /// `NIL` (untracked) entry is legal only for a body that is not a closure
    /// instance — a top-level program, module body, or eval'd form. `NIL`
    /// between calls; never read except by the immediately following entry.
    pub(crate) pending_entry_closure: Value,
    /// One-shot "the caller parks this activation's frame on an error exit", set
    /// immediately before entering a body via `execute_bytecode_saving_stack`,
    /// which takes it (resetting to `false`) at entry. A parked frame is
    /// replayable — the restarts system resumes an `:error` fiber into it — so the
    /// releases it still owes stay owed and the abandoned-frame walk must not run
    /// them (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
    /// it still owes"). `do_fiber_first_resume` is the one entrant that parks an
    /// error frame; taken at entry, so the frames that body CALLS still walk.
    pub(crate) pending_error_park: bool,
    /// One-shot parent-wiring override for the next `with_child_fiber`.
    /// Set by the trampoline descent in `do_fiber_resume` from
    /// `PendingFiberResume::parent`; consumed (taken) by `with_child_fiber`
    /// in place of its default "currently active fiber" parent wiring.
    pub(crate) trampoline_parent_override: Option<(FiberHandle, Option<Value>)>,
    /// Source location of the instruction that produced the current error.
    /// Resolved by the dispatch loop using the current closure's LocationMap.
    /// Reset to None at each translation boundary entry.
    /// Guarded by is_none() — innermost (origin) location wins over outer
    /// call sites. This also protects against fiber error propagation
    /// overwriting the child fiber's error origin.
    pub(crate) error_loc: Option<SourceLoc>,
    /// Reason carried by the most recent uncaught `:gated` error to propagate
    /// out of `execute_proto`. A loud `(gate! …)` whose condition is unmet
    /// raises `{:error :gated :reason …}`; when that escapes to the top level
    /// uncaught, it is an intentional SKIP, not a failure. The top-level driver
    /// (`run_source`) reads this to exit 0 with a notice instead of erroring.
    /// Set to `Some`/`None` on every uncaught error (so a stale reason from a
    /// caught gate never lingers); only meaningful when execution returned Err.
    pub(crate) gated_exit_reason: Option<String>,
    /// The backend tier currently executing under `compile/run-on`
    /// (`"bytecode"`, `"jit"`, `"wasm"`, `"mlir-cpu"`), read by `(vm/tier)` /
    /// `(backend? :tier)` so a closure compiled once and dispatched to several
    /// tiers learns which one it is running on at runtime. `dispatch_compile_run_on`
    /// saves, sets, and restores it around the forced-tier call; `"bytecode"`
    /// otherwise. A `VM` field (not shared state) so two instances never collide.
    pub(crate) active_tier: &'static str,
    /// When set, `(exit)` emits a catchable `{:error :exited :code N}` instead of
    /// terminating the process. Toggled by `(sys/trap-exit! on)`. The test runner
    /// brackets each test with it so a test's `(exit)` records a result and the run
    /// continues. A `VM` field: one VM per worker thread, so per-VM equals
    /// per-thread, keeping the trap scoped to the calling OS thread (the runner's
    /// own `exit`, on a VM with the trap unset, still terminates the process).
    pub(crate) exit_trapped: bool,
    /// JIT code cache: bytecode pointer → pinned entry (see `JitCacheEntry`
    /// and docs/impl/jit.md § "Cache identity"). Write through
    /// `install_jit_code`; read through `jit_code_for`.
    #[cfg(feature = "jit")]
    pub jit_cache: FxHashMap<*const u8, JitCacheEntry>,
    /// Background JIT compilation worker (spawned lazily).
    #[cfg(feature = "jit")]
    pub(crate) jit_worker: Option<crate::jit::worker::JitWorker>,
    /// Compilations in flight on the worker, keyed by bytecode address. The
    /// value pins the keyed allocation from submission until the result
    /// installs (docs/impl/jit.md § "Cache identity"); the pin then moves
    /// into `jit_cache` or `jit_rejections`.
    #[cfg(feature = "jit")]
    pub(crate) jit_pending: FxHashMap<usize, crate::value::ClosureTemplate>,
    /// Documentation for all named forms (primitives, special forms, macros).
    /// Keyed by name string for direct lookup via `doc` and `vm/primitive-meta`.
    pub docs: HashMap<String, Doc>,
    /// JIT rejection log: bytecode pointer → rejection info.
    /// Records first rejection per closure template. Used by
    /// `(jit/rejections)` primitive and `--stats` CLI flag.
    #[cfg(feature = "jit")]
    pub jit_rejections: FxHashMap<*const u8, JitRejectionInfo>,
    /// Per-template count of background JIT compilations submitted.
    /// Incremented on every `submit_jit_task`. The negative-cache
    /// invariant (see docs/impl/jit.md) holds this at 1 for a rejected
    /// function regardless of call count; a regression shows up here as
    /// an unbounded `:attempts` in `(jit/rejections)`.
    #[cfg(feature = "jit")]
    pub jit_compile_attempts: FxHashMap<*const u8, usize>,
    /// Cached Expander for runtime `eval`. Avoids re-loading the prelude
    /// on every eval call. Taken out during eval, put back after.
    pub eval_expander: Option<crate::syntax::Expander>,
    /// User-provided command-line arguments, from everything after `--`
    /// in the argv passed to the elle binary. Empty if no `--` was given.
    /// Set by `main.rs` before the file-execution loop. Read by `sys/args`.
    pub user_args: Vec<String>,
    /// The source argument: the script file path, `"-"` for stdin, or `""`
    /// in REPL mode. Set by `main.rs` at the same point as `user_args`.
    /// Read by `sys/argv`. Empty string means REPL mode.
    pub source_arg: String,
    /// Lazy WASM compilation tier. When `--wasm=N`, hot closures are
    /// compiled to per-closure WASM modules and dispatched through Wasmtime.
    #[cfg(feature = "wasm")]
    pub wasm_tier: Option<crate::wasm::lazy::WasmTier>,
    /// Closures that failed WASM compilation (contain MakeClosure, TailCall, etc.)
    #[cfg(feature = "wasm")]
    pub(crate) wasm_rejections: FxHashMap<*const u8, ()>,
    /// Whether MLIR compilation is enabled (runtime gate).
    /// Controlled by `--mlir=` CLI flag and `(vm/config-set :mlir ...)`.
    #[cfg(feature = "mlir")]
    pub(crate) mlir_enabled: bool,
    /// MLIR compilation cache for GPU-eligible functions.
    /// Lazily initialized on first GPU-eligible call.
    #[cfg(feature = "mlir")]
    pub(crate) mlir_cache: Option<crate::mlir::MlirCache>,
}

mod decode;
mod format;
mod lifecycle;
mod region;
mod resume;

/// Where a tier keeps the local slots the abandoned-frame walk reads — named
/// here because the compiled entry (`elle_jit_release_abandoned_frame`) picks
/// the variant the interpreter never uses. That entry is the only reader, so a
/// build without the compiled tier does not name the type here at all.
#[cfg(feature = "jit")]
pub(crate) use region::FrameLocals;

impl VM {
    /// Access the root heap.
    #[inline]
    pub fn heap(&mut self) -> &mut crate::value::fiberheap::FiberHeap {
        unsafe { &mut *self.heap_ptr }
    }

    /// Point this VM at its owning instance's compile context. Set by
    /// `RuntimeCore` (the boxed `CompileCtx` has a stable address). See
    /// `compile_ctx_ptr`.
    pub fn set_compile_ctx(&mut self, ctx: *mut crate::pipeline::CompileCtx) {
        self.compile_ctx_ptr = ctx;
    }

    /// Point this VM at its owning instance's symbol table. Set by `RuntimeCore`
    /// (the boxed `SymbolTable` has a stable address). See `symbols_ptr`.
    pub fn set_symbols(&mut self, symbols: *mut crate::symbol::SymbolTable) {
        self.symbols_ptr = symbols;
    }

    /// The owning instance's symbol table, for runtime name interning/resolution.
    /// Returns `None` for a bare VM (no `RuntimeCore`); the readers treat that as
    /// "symbol table unavailable". The borrow is sound by the same contract as
    /// `heap()`/`compile_ctx()`: the `SymbolTable` is a disjoint allocation owned
    /// by the `RuntimeCore` that outlives the VM, reborrowed per call (the VM does
    /// not touch it during the synchronous read).
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn symbols(&self) -> Option<&mut crate::symbol::SymbolTable> {
        if self.symbols_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.symbols_ptr })
        }
    }

    /// The spelling of a keyword value, through this instance's memo and the
    /// static vocabulary. `None` if `v` is not a keyword or the spelling was
    /// never learned.
    pub(crate) fn keyword_spelling(&self, v: crate::value::Value) -> Option<String> {
        let hash = v.keyword_hash()?;
        crate::value::keyword::resolve_keyword_name(self.symbols().map(|s| &*s), hash)
            .map(str::to_string)
    }

    /// The owning instance's compile context, for the runtime `eval`
    /// instruction. Returns `None` for a bare VM (no `RuntimeCore`), in which
    /// case `(eval …)` of macro-using code is unsupported. The borrow is sound
    /// by the same contract as `heap()`: the `CompileCtx` is a disjoint
    /// allocation owned by the `RuntimeCore` that outlives the VM.
    #[allow(clippy::mut_from_ref)]
    pub(crate) fn compile_ctx(&self) -> Option<&mut crate::pipeline::CompileCtx> {
        if self.compile_ctx_ptr.is_null() {
            None
        } else {
            Some(unsafe { &mut *self.compile_ctx_ptr })
        }
    }

    /// Take the reason of the most recent uncaught `:gated` error, if the last
    /// failed execution terminated via a loud gate. Consumes it (returns None on
    /// a second call). The top-level driver uses this to exit 0 with a skip
    /// notice instead of reporting a failure. See `gated_exit_reason`.
    pub fn take_gated_exit_reason(&mut self) -> Option<String> {
        self.gated_exit_reason.take()
    }

    /// Check a signal against a squelch mask. If the signal is squelched,
    /// sets the fiber to a signal-violation error and returns `true`.
    /// Callers handle any additional side effects (stack push, call_stack pop, etc.).
    ///
    /// Which bits a boundary enforces is `signals::squelched_bits`' answer, shared
    /// with the JIT's inlined checks.
    pub(crate) fn enforce_squelch(&mut self, bits: SignalBits, mask: SignalBits) -> bool {
        let squelched = crate::signals::squelched_bits(bits, mask);
        if squelched.is_empty() {
            return false;
        }
        // The park this boundary ends is the fiber's own live signal here: every
        // site reaching the boundary through this predicate is still holding it
        // (the two that are not pass their own — see `squelch_violation`).
        let err = self.squelch_violation(squelched, self.fiber.signal);
        self.fiber.signal = Some((crate::value::SIG_ERROR, err));
        true
    }

    /// Build the `signal-violation` error a squelch/attune boundary raises for
    /// `squelched`, and discard the suspended frames the boundary abandons.
    ///
    /// The error value is returned rather than stored: each enforcement site
    /// delivers it differently — the interpreter sets `fiber.signal`, the
    /// `compile/run-on` entry returns it as the call's result. `squelched` must
    /// be non-empty, the answer `signals::squelched_bits` gives.
    ///
    /// `parked` is the signal whose park this boundary ends, named by the site
    /// rather than read from `fiber.signal`: two sites reach here through
    /// `invoke_closure_jit`, which restores the CALLER's signal before it asks
    /// the boundary's question and holds the parked one in a local. What the
    /// park owed is released against it (docs/impl/region/owner.md § "A boundary
    /// ends a park with no reader and no install").
    pub(crate) fn squelch_violation(
        &mut self,
        squelched: SignalBits,
        parked: Option<(SignalBits, Value)>,
    ) -> Value {
        let squelched_str = crate::signals::registry::format_bits(squelched);
        let err = self.escaping_error(
            "signal-violation",
            format!("squelch: signal {} caught at boundary", squelched_str),
        );
        // The error the boundary raises is what leaves with the signal, so it is
        // the payload the discard's own releases must leave standing — built in a
        // fresh region of its own, so in practice it exempts nothing and the
        // abandoned frames' tables run in full.
        self.discard_suspended_frames(err, parked);
        err
    }

    /// Discard the LIVE fiber's suspended frames (squelch / abort) — the
    /// chokepoint for abandoning suspended work while the fiber runs on, the
    /// discard counterpart of `resume_suspended` (docs/impl/region/owner.md
    /// § "A discard runs what the abandoned frames owed"; a fiber reaching a
    /// TERMINAL state instead releases through
    /// `vm::fiber::take_fiber_owned`/`release_fiber_owned`, which also frees the
    /// fiber owner node this discard leaves alone — the fiber survives a squelch
    /// and may still own it).
    ///
    /// The discarded frames' continuations will never run, so every release that
    /// lived in one is at its last chance here. What each frame owes it carries
    /// itself, in three readings [`crate::value::fiber::ParkedDues`] collects
    /// together: its parked owner node and the releases its activation took over
    /// from its own frame-replacing tail calls (both MOVED into the frame at the
    /// suspend — the record's only home, so no completion or resume can reach
    /// them a second time), and the releases its two emitter-recorded tables name,
    /// read against its saved locals and its saved activation map.
    ///
    /// Each reading names regions this chokepoint is entitled to release: a
    /// node's members are exactly the regions the inference proved externally
    /// unique and moved in through `AdoptIntoActivation`, a deferred region is one
    /// the compiler itself named as this activation's to release, and a table
    /// entry is a release the executing function emitted for its own slot with a
    /// receipt that says it did not run. The fiber's survival is not part of that
    /// reading: a frame's saved stack and saved map were taken and cloned at its
    /// own park, and its activation returned before this point, so no live frame
    /// shares the state the receipts are read against.
    ///
    /// What stays refused is a blanket release of the rest of the frame's
    /// `activation_region_map`. It is a borrowed view carrying no receipt, and a
    /// region it names can still be live in an outer, non-discarded frame or in
    /// the activation that catches the squelch. Those regions stay leaked on this
    /// path until an ownership cut adopts them (UAF-safe, bounded per discard).
    ///
    /// A fourth reading answers for the PARK rather than for the frames, and it
    /// is the delivery ledger's: this exit is neither the reader that consumes a
    /// park's delivery retain nor the install that releases a runtime-built
    /// payload, so both are owed here (docs/impl/region/owner.md § "A boundary
    /// ends a park with no reader and no install").
    ///
    /// `payload` is the value the exit leaves with — the boundary's own
    /// `signal-violation` error — whose region no release here may take, on the
    /// same reading the abandoned-frame walk makes
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). `parked` is the signal whose park this ends, named by the
    /// enforcement site (see `squelch_violation`).
    pub(crate) fn discard_suspended_frames(
        &mut self,
        payload: Value,
        parked: Option<(SignalBits, Value)>,
    ) {
        if let Some(frames) = self.fiber.suspended.take() {
            let dues = crate::value::fiber::ParkedDues::of(frames);
            let protect = Some(payload).filter(|v| !self.fiber.delivery.mint_names(*v));
            let heap = unsafe { &mut *self.heap_ptr };
            crate::vm::fiber::release_parked_dues(heap, dues, protect, None);
        }
        // The park's own references run after the tables above, which still find
        // the payload's region live: a body-allocated payload's own release is
        // one of those entries, and the delivery retain dropped here is what
        // keeps it from being the region's last.
        let heap = unsafe { &mut *self.heap_ptr };
        crate::vm::fiber::release_abandoned_park(heap, &mut self.fiber.delivery, parked);
        // The abandoned park's funding has no consumer — the delivery funnel
        // that would have taken it will never run for these frames.
        self.fiber.delivery.discharge();
    }

    /// A host that drives a thunk on the CURRENT fiber (`eval`, `import`,
    /// `arena/allocs`, `compile/run-on`, the root driver) refuses a
    /// suspend-class signal it cannot host: it extracts the signal as a value
    /// or reports it, and the fiber runs on. The park that raised the signal
    /// is dead at that moment, so its funding record must not survive into
    /// the fiber's next park — the delivery funnel that would consume it
    /// belongs to a resume no host will ever run (docs/impl/region/owner.md
    /// § "A park names its funding in the delivery ledger"). A no-op for a
    /// completion, an error (an `:error` fiber is resumable and its records
    /// are identity-gated), a halt, or the switch trampoline — none of those
    /// abandons a suspend-class park.
    pub(crate) fn abandon_hosted_park(&mut self, bits: SignalBits) {
        use crate::value::{SIG_ERROR, SIG_HALT, SIG_SWITCH};
        if bits.is_empty()
            || bits.intersects(SIG_ERROR)
            || bits.intersects(SIG_HALT)
            || bits == SIG_SWITCH
        {
            return;
        }
        self.fiber.delivery.discharge();
    }

    /// Record a closure call and return whether it's "hot" (called N+ times,
    /// where N is `jit_hotness_threshold`, default 10, set via `--jit=N`).
    pub fn record_closure_call(&mut self, bytecode_ptr: *const u8) -> bool {
        let count = self.closure_call_counts.entry(bytecode_ptr).or_insert(0);
        *count += 1;
        *count >= self.runtime_config.jit.threshold()
    }

    /// Get call count for a closure
    pub fn get_closure_call_count(&self, bytecode_ptr: *const u8) -> usize {
        self.closure_call_counts
            .get(&bytecode_ptr)
            .copied()
            .unwrap_or(0)
    }

    /// Check if a module is currently being loaded (circular dependency).
    pub fn is_module_loading(&self, module_path: &str) -> bool {
        self.loading_modules.contains(module_path)
    }

    /// Mark a module as currently loading (for circular-import detection).
    pub fn mark_module_loading(&mut self, module_path: String) {
        self.loading_modules.insert(module_path);
    }

    /// Unmark a module as loading (after execution completes).
    pub fn unmark_module_loading(&mut self, module_path: &str) {
        self.loading_modules.remove(module_path);
    }

    /// Get the frame base for the current call frame
    /// Returns 0 if no call frame (top-level execution)
    pub fn current_frame_base(&self) -> usize {
        self.fiber
            .call_stack
            .last()
            .map(|f| f.frame_base)
            .unwrap_or(0)
    }

    /// Push a synthetic trace frame for `name`, whose call site is at `ip` in a
    /// code object carrying `location_map`. Both code objects are built here
    /// from blueprints, so a trace test exercises the same path a real call
    /// takes.
    #[cfg(test)]
    fn push_call_frame(&mut self, name: &str, ip: usize, location_map: crate::error::LocationMap) {
        let mut callee =
            crate::value::TemplateProto::new(Vec::new(), crate::value::Arity::Exact(0), Vec::new());
        callee.name = Some(name.to_string());
        let mut caller =
            crate::value::TemplateProto::new(Vec::new(), crate::value::Arity::Exact(0), Vec::new());
        caller.location_map = location_map;
        let heap = self.heap();
        let callee = crate::value::ClosureTemplate::for_proto(heap, &Rc::new(callee)).code();
        let caller = crate::value::ClosureTemplate::for_proto(heap, &Rc::new(caller)).code();

        let frame_base = self.fiber.stack.len();
        self.fiber.call_depth += 1;
        self.fiber.call_stack.push(crate::value::fiber::CallFrame {
            callee,
            caller,
            ip,
            frame_base,
        });
    }
}

#[cfg(test)]
mod tests;
