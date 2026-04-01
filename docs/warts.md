# Warts

### RC without cycle collection

No GC, no weak-ref discipline for user values, no cycle detector. A
closure that captures its own binding (natural in any recursive local
function via letrec) creates an Rc cycle that lives forever. Long-running
programs using self-referencing actors leak silently with no diagnostic.
The fiber tree uses WeakFiberHandle for parent pointers - but user data
has no such protection. Fix is a tracing GC or cycle collector.

### *mut VM in the compilation pipeline

`pipeline/cache.rs` extracts a `*mut VM` raw pointer from a
thread-local RefCell, releases the borrow, passes the raw pointer to
callers. The invariant ("pipeline functions are not re-entrant") is
maintained by convention, not by the type system. One refactor that
holds the borrow too long and you get a runtime panic during macro
expansion.

### SyncBackend + AsyncBackend duplication

Port buffering logic is ~70% duplicated between the sync and async
backends. The shared buffering code should be factored out.
(Process spawning was deduplicated into `SpawnRequest::spawn_to_struct`.)

### Thread-local singletons in multiple modules

`context.rs`, `primitives/list/mod.rs`, `primitives/registration.rs`,
`ffi/callback.rs` each have independent `thread_local!` state. No
unified strategy, no safety wrapper.
