# Warts

### RC without cycle collection

No GC, no weak-ref discipline for user values, no cycle detector. A
closure that captures its own binding (natural in any recursive local
function via letrec) creates an Rc cycle that lives forever. Long-running
programs using self-referencing actors leak silently with no diagnostic.
The fiber tree uses WeakFiberHandle for parent pointers - but user data
has no such protection. Fix is a tracing GC or cycle collector.

### Thread-local singletons in multiple modules

`context.rs` and `ffi/callback.rs` each have independent `thread_local!`
state.  `primitives/registration.rs` caches primitive metadata in
another.  No unified strategy, no safety wrapper.

(The former `primitives/list` duplicate `SYMBOL_TABLE` was consolidated
into `context.rs`.  The former `*mut VM` in `pipeline/cache.rs` was
replaced with a closure-based API that holds the `RefCell` borrow.)

### SyncBackend + AsyncBackend duplication

Port buffering logic is ~70% duplicated between the sync and async
backends. The shared buffering code should be factored out.
(Process spawning was deduplicated into `SpawnRequest::spawn_to_struct`.)


