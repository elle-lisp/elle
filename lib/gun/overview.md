# gun

<!-- audited: 2026-10-11 -->

The submodules behind [gun.lisp](../gun.lisp): states, values, the HAM verdict, the in-memory graph and the stores.

[gun.md](../gun.md) holds what a caller of the module sees. This file holds how
the module divides. Each file's header says what it exports, and `(doc name)`
carries a function's arguments.

| File | Purpose |
|------|---------|
| [state.lisp](state.lisp) | The hybrid logical clock, and the only comparison of two states |
| [value.lisp](value.lisp) | What a value may be, links, and the encoding that stores and orders values |
| [ham.lisp](ham.lisp) | The HAM verdict on one incoming field against the current one |
| [graph.lisp](graph.lisp) | Souls, the in-memory nodes, the merge that answers a diff, and the flattening of a nested struct |
| [store.lisp](store.lisp) | The memory store and the Redis store, behind one interface |

A submodule takes the submodules it needs as arguments, so loading one by hand
follows the order [gun.lisp](../gun.lisp) uses:

```lisp
(def state ((import "std/gun/state")))
(def value ((import "std/gun/value")))
(def ham ((import "std/gun/ham") :state state :value value))
(def graph ((import "std/gun/graph") :state state :value value :ham ham))
(def store ((import "std/gun/store") :state state :value value))
(assert (= (value:encode (value:link "users/alice")) "l:users/alice"))
(assert (= (graph:child-soul "x" "a/b") "x/a%2Fb"))
```

`std/gun/store` takes `std/redis` as `:redis`, and builds a Redis store only
when it has one.
