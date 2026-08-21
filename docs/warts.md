# Warts

### Rc in mutable collections

Closure environments are now `RegionSlice<Value>` in the bump arena —
self-referencing closures (letrec recursion) create arena pointer cycles,
not Rc cycles, and are reclaimed by scope exit or fiber death.

Mutable collections (`@array`, `@struct`, `@set`, `@string`, `@bytes`)
and `CaptureCell` still use `Rc<RefCell<_>>`. A mutable container that
stores a reference to itself (e.g., an `@array` that `push`es itself)
creates an Rc cycle. This is rare in practice — it requires explicit
self-insertion, not the natural letrec pattern that was the original
concern.
