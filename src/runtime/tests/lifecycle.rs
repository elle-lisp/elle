use super::*;
/// The load-bearing correctness test: teardown frees a *registered* root by
/// RC reaching zero, and leaves an *unregistered* live region alone. The
/// asymmetry is the proof that the sweep is RC-driven (it drops roots and
/// cascades) and NOT iterate-and-free (which would have freed both).
///
/// Counterfactual: if `teardown` walked the region table freeing entries,
/// the unregistered region would also be gone and the second assertion would
/// fail; if it freed nothing, the first assertion would fail.
#[test]
fn teardown_frees_registered_root_only_via_rc() {
    // `without_stdlib` keeps the region baseline clean and deterministic.
    let mut rt = Runtime::without_stdlib();

    // A root: a leak-free value in its own fresh region (rc starts at 1).
    let (_root_val, root_region) = alloc_in_fresh_region(rt.heap(), cons());
    register_process_root_region(rt.heap(), root_region);

    // A non-root: an equivalent value in another fresh region, NOT
    // registered. Nothing else references it (rc 1), so only an
    // iterate-and-free teardown would reclaim it.
    let (_orphan_val, orphan_region) = alloc_in_fresh_region(rt.heap(), cons());

    assert_eq!(
        region_rc(rt.heap(), root_region),
        1,
        "root region live before teardown"
    );
    assert_eq!(
        region_rc(rt.heap(), orphan_region),
        1,
        "orphan region live before teardown"
    );

    let report = rt.teardown();

    assert!(
        report.roots_released >= 1,
        "teardown must release the registered root (released={})",
        report.roots_released
    );
    assert_eq!(
        region_rc(rt.heap(), root_region),
        0,
        "registered root must be freed by RC reaching zero"
    );
    assert_eq!(
        region_rc(rt.heap(), orphan_region),
        1,
        "an UNregistered live region must survive — teardown is RC-driven, \
             not iterate-and-free (this is the counterfactual that fails if the \
             sweep ever walks the region table freeing entries)"
    );
}

/// Counterfactual for the coexistence effort: two embedded Elle instances on one
/// thread, doing interleaved **compute**, must be isolated. (io trading between
/// instances is not covered; this pins compute only.) The litmus: a site is
/// correct iff it behaves correctly with two instances interleaved on one
/// thread — any read of state shared between the instances fails it.
///
/// Each instance maintains its own top-level binding `x` via the persistent-def
/// path an embedded REPL uses (`compile_file_repl` +
/// `CompileCtx::register_repl_binding`). The binding, and the stdlib `ev/run` the
/// scheduler resolves, both live in the instance's own `CompileCtx`; a
/// shared compile cache would let instance B's `(def x …)` overwrite
/// instance A's, so each instance must read back only its own `x`.
#[test]
fn two_instances_interleaved_defs_are_isolated() {
    use crate::pipeline::compile_file_repl;
    use crate::signals::Signal;

    fn def(rt: &mut Runtime, name: &str, src: &str) {
        let (vm, symbols, cctx) = rt.parts();
        let (result, _expander) =
            compile_file_repl(src, symbols, cctx, "<embed>").expect("def compiles");
        // A simple `(def x V)` returns V (the letrec body is the bound name).
        let value = vm
            .execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("def runs");
        let sym_id = symbols.intern(name);
        cctx.register_repl_binding(
            unsafe { &mut *vm.heap_ptr },
            sym_id,
            value,
            Signal::silent(),
            None,
        );
    }

    fn read_int(rt: &mut Runtime, src: &str) -> i64 {
        let (vm, symbols, cctx) = rt.parts();
        let (result, _expander) =
            compile_file_repl(src, symbols, cctx, "<embed>").expect("read compiles");
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("read runs")
            .as_int()
            .expect("read yields an int")
    }

    let mut a = Runtime::new();
    let mut b = Runtime::new();

    // Interleave: each instance defines its own `x`, b after a.
    def(&mut a, "x", "(def x 1)");
    def(&mut b, "x", "(def x 2)");

    // Each instance must read back its OWN binding.
    assert_eq!(
        read_int(&mut a, "x"),
        1,
        "instance a must see its own x=1, not b's x=2 (shared compilation cache)"
    );
    assert_eq!(read_int(&mut b, "x"), 2, "instance b must see its own x=2");
}

/// Counterfactual for the vm-context axis: two embedded instances on one thread
/// must each read their OWN VM's state, never state shared between instances.
///
/// `(sys/argv)` reads `vm.source_arg` / `vm.user_args`. When the VM is reached
/// through a slot shared by every instance on the thread, that slot points at
/// whichever `Runtime` was constructed last (here `b`), so running `(sys/argv)`
/// through `a` reads `b`'s args. Reaching the call's own driving VM via
/// `ctx.vm()` instead makes each instance read its own — a site is correct iff
/// it behaves correctly with two instances interleaved on one thread.
#[test]
fn two_instances_read_their_own_vm_args() {
    use crate::pipeline::compile_file_repl;

    fn first_argv(rt: &mut Runtime, src: &str) -> Option<String> {
        let (vm, symbols, cctx) = rt.parts();
        let (result, _expander) =
            compile_file_repl(src, symbols, cctx, "<embed>").expect("compiles");
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .expect("runs")
            .with_string(|s| s.to_string())
    }

    let mut a = Runtime::new();
    let mut b = Runtime::new();

    // Distinct script paths: `(sys/argv)`'s element 0 is `vm.source_arg`.
    a.vm().source_arg = "a.lisp".to_string();
    b.vm().source_arg = "b.lisp".to_string();

    assert_eq!(
        first_argv(&mut a, "(first (sys/argv))").as_deref(),
        Some("a.lisp"),
        "instance a's (sys/argv) must read a's own VM source_arg, not b's \
         (VM state must not be shared between instances)"
    );
    assert_eq!(
        first_argv(&mut b, "(first (sys/argv))").as_deref(),
        Some("b.lisp"),
        "instance b's (sys/argv) must read b's own VM source_arg"
    );
}

/// Counterfactual for the symbol axis: two embedded instances on one thread must
/// each resolve symbol NAMES through their OWN symbol table.
///
/// `(string 'sym)` interns `sym` into the COMPILING instance's table (at a fresh
/// id past the shared stdlib), then at runtime resolves that id back to a name. If
/// runtime resolution read a single per-thread table shared by both instances, it
/// would read whichever `Runtime` was built last (`b`), so instance `a`'s fresh
/// id — valid only in `a`'s table — is out of range in `b`'s table and
/// `(string …)` fails. Reaching the call's own driving VM's table via
/// `ctx.vm().symbols()` makes each instance resolve its own: a site is correct iff
/// it behaves correctly with two instances interleaved on one thread.
#[test]
fn two_instances_resolve_their_own_symbols() {
    use crate::pipeline::compile_file_repl;

    fn to_string(rt: &mut Runtime, src: &str) -> Option<String> {
        let (vm, symbols, cctx) = rt.parts();
        let (result, _expander) = compile_file_repl(src, symbols, cctx, "<embed>").ok()?;
        vm.execute_scheduled(&result.bytecode, symbols, cctx)
            .ok()?
            .with_string(|s| s.to_string())
    }

    let mut a = Runtime::new();
    let mut b = Runtime::new();

    // `a`'s fresh symbol id is the discriminator: a single shared table
    // (resolving against `b`, built last) cannot resolve it.
    assert_eq!(
        to_string(&mut a, "(string 'alpha_only_in_a)").as_deref(),
        Some("alpha_only_in_a"),
        "instance a must resolve its own symbol via its own table, not b's \
         (the symbol table must not be shared between instances)"
    );
    assert_eq!(
        to_string(&mut b, "(string 'beta_only_in_b)").as_deref(),
        Some("beta_only_in_b"),
        "instance b must resolve its own symbol via its own table"
    );
}

/// Counterfactual for the heap axis (tls.md § Acceptance criterion): two
/// embedded instances on one thread each allocate in their OWN heap, and
/// neither sees the other's regions. A site is correct iff it behaves correctly
/// with two instances interleaved on one thread; a shared per-*thread* heap
/// fails it, because on one thread the two instances share that slot.
///
/// The discriminator is a region-count delta. Allocating regions in b's heap
/// must not change a's region count: with separate stores a's count is
/// untouched (delta 0), but with ONE shared store every region b mints is also
/// in a's store, so a's count rises by exactly the number b allocated (RED).
/// A per-id `region_rc`/free probe cannot catch this — a shared store hands the
/// two instances *distinct* ids, so it masks the bug; the aggregate count does
/// not.
#[test]
fn two_instances_each_allocate_in_their_own_heap() {
    let mut a = Runtime::without_stdlib();
    let mut b = Runtime::without_stdlib();

    let a_before = a.heap().active_region_count();
    let b_before = b.heap().active_region_count();

    // Allocate several fresh regions on b's heap only.
    const N: usize = 5;
    for _ in 0..N {
        let h = b.heap();
        let r = h.new_runtime_region();
        let _ = h.alloc_in_region(cons(), r);
    }

    let a_after = a.heap().active_region_count();
    let b_after = b.heap().active_region_count();

    assert_eq!(
        a_after, a_before,
        "allocating {N} regions on b's heap must not change a's region count \
         (a {a_before} → {a_after}); a shared per-thread heap puts b's regions \
         in a's store too",
    );
    assert_eq!(
        b_after,
        b_before + N,
        "b's own heap must grow by exactly the {N} regions allocated on it \
         (b {b_before} → {b_after})",
    );
}

/// Teardown is idempotent: a second call releases no further roots and never
/// underflows or re-mints. (A double-free would panic in debug via the
/// regionstore phantom/double-free assert.)
#[test]
fn teardown_is_idempotent() {
    let mut rt = Runtime::without_stdlib();
    let (_v, r) = alloc_in_fresh_region(rt.heap(), cons());
    register_process_root_region(rt.heap(), r);

    let first = rt.teardown();
    assert!(first.roots_released >= 1);
    let second = rt.teardown();
    assert_eq!(
        second.roots_released, 0,
        "second teardown must release nothing (registry already drained)"
    );
    assert!(second.live_regions <= first.live_regions);
}
