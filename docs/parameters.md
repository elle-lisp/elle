# Dynamic Parameters

Dynamic parameters are fiber-local variables with scoped rebinding.
They provide dynamic scope without breaking lexical scope rules.

## Creating parameters

```lisp
(def *verbose* (make-parameter false))
(*verbose*)                # => false
(parameter? *verbose*)     # => true
```

## Scoped rebinding

`parameterize` temporarily overrides a parameter's value for the
dynamic extent of its body.

```lisp
(def *indent* (make-parameter 0))

(defn show-level []
  (println (string/repeat "  " (*indent*)) "level " (*indent*)))

(show-level)               # "level 0"

(parameterize ((*indent* 1))
  (show-level)             # "  level 1"
  (parameterize ((*indent* 2))
    (show-level)))         # "    level 2"

(show-level)               # "level 0" (restored)
```

## Built-in parameters

`*stdout*` and `*stderr*` are dynamic parameters. Rebinding them
redirects output:

```lisp
# (parameterize ((*stdout* my-port))
#   (println "goes to my-port, not terminal"))
```

## Fiber inheritance

Parameters are *fiber-local*: each fiber has its own `param_frames`
stack. A fresh fiber created by `fiber/new` automatically inherits a
snapshot of the creating fiber's current parameter bindings — the
snapshot is taken at creation time, not at resume time, so the child
sees what the parent had bound the moment `fiber/new` was called.

```lisp
(def *colour* (parameter :default))

(def f (parameterize ((*colour* :red))
         (fiber/new (fn () (*colour*)) 1)))

# parameterize has unwound here — `(*colour*)` would read :default —
# but the fiber captured :red at creation:
(fiber/resume f nil)
(fiber/value f)              # => :red
```

This matters for `ev/spawn`: the spawning fiber may finish (and its
`parameterize` blocks may unwind) long before the scheduler resumes
the spawned fiber. Without creation-time snapshotting the child would
observe whatever bindings the scheduler happens to hold at resume time
— typically nothing useful.

The snapshot is a *copy*. Reparameterizing in the creator after
spawning does not affect already-created children, and the resumer's
bindings do not override the child's snapshot:

```lisp
(def *x* (parameter :default))

(def f (parameterize ((*x* :captured))
         (fiber/new (fn () (*x*)) 1)))

(parameterize ((*x* :resumer-has-this))
  (fiber/resume f nil))
(fiber/value f)              # => :captured
```

Inheritance is transitive — a fiber that spawns another fiber passes
its snapshot down. There is no opt-in or opt-out: if you want fresh
bindings inside the fiber, wrap its body in `parameterize`.

---

## See also

- [io.md](io.md) — ports and output
- [bindings.md](bindings.md) — lexical bindings
- [concurrency.md](concurrency.md) — fibers, `ev/spawn`, and scheduling
