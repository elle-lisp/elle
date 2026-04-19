# Warts

### Rc in mutable collections

Closure environments are now `InlineSlice<Value>` in the bump arena —
self-referencing closures (letrec recursion) create arena pointer cycles,
not Rc cycles, and are reclaimed by scope exit or fiber death.

Mutable collections (`@array`, `@struct`, `@set`, `@string`, `@bytes`)
and `CaptureCell` still use `Rc<RefCell<_>>`. A mutable container that
stores a reference to itself (e.g., an `@array` that `push`es itself)
creates an Rc cycle. This is rare in practice — it requires explicit
self-insertion, not the natural letrec pattern that was the original
concern.

### Thread-local singletons in multiple modules

`context.rs` and `ffi/callback.rs` each have independent `thread_local!`
state.  `primitives/registration.rs` caches primitive metadata in
another.  No unified strategy, no safety wrapper.

(The former `primitives/list` duplicate `SYMBOL_TABLE` was consolidated
into `context.rs`.  The former `*mut VM` in `pipeline/cache.rs` was
replaced with a closure-based API that holds the `RefCell` borrow.)




