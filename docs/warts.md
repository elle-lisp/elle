# Warts

### Cyclic mutable structures crash the process

`(def a @[]) (push a a) (println (get a 0))` — Rust stack overflow,
SIGABRT, process dead. Same for `(= a a)` and `(hash a)`. The Rust
`Display`, `PartialEq`, and `Hash` impls recurse without cycle
detection. No `protect`, no `try/catch`, no signal can save you. Fix
requires a visited-set or thread-local cycle guard in traversal.

### RC without cycle collection

No GC, no weak-ref discipline for user values, no cycle detector. A
closure that captures its own binding (natural in any recursive local
function via letrec) creates an Rc cycle that lives forever. Long-running
programs using self-referencing actors leak silently with no diagnostic.
The fiber tree uses WeakFiberHandle for parent pointers — but user data
has no such protection. Fix is a tracing GC or cycle collector.

### *mut VM in the compilation pipeline

`pipeline/cache.rs` extracts a `*mut VM` raw pointer from a
thread-local RefCell, releases the borrow, passes the raw pointer to
callers. The invariant ("pipeline functions are not re-entrant") is
maintained by convention, not by the type system. One refactor that
holds the borrow too long and you get a runtime panic during macro
expansion.

### SyncBackend + AsyncBackend duplication

Process spawning is ~95% duplicated — identical Command::new(), env/cwd
setup, stdio configuration, pipe conversion. Port buffering logic (~70%
duplicate). The shared setup code should be factored out.

### Plugin init boilerplate across 30+ plugins

Every plugin crate copy-pastes the same `elle_plugin_init`: create
BTreeMap, iterate PRIMITIVES, strip prefix, register, build struct. No
shared macro or `elle::plugin::init!`. ~1,000 lines of duplication
across the workspace.

### Thread-local singletons in multiple modules

`context.rs`, `primitives/list/mod.rs`, `primitives/registration.rs`,
`ffi/callback.rs` each have independent `thread_local!` state. No
unified strategy, no safety wrapper.
