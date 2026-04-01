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

---

## See also

- [io.md](io.md) — ports and output
- [bindings.md](bindings.md) — lexical bindings
