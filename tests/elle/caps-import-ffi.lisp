(elle/epoch 12)
# audited: 2026-09-21
# ── import requires :ffi to load a native library (#1074) ──────────────
#
# `import` declares `:fs` for the read, so denying `:ffi` did not stop loading
# a shared library — which runs its `elle_plugin_init`, foreign code the `:ffi`
# capability exists to withhold. The requirement now rides the path argument: a
# `.so`/`.dylib`/`.dll` spec requires `:ffi`, a `.lisp` module does not. This is
# the same bits_from_args gate `io/submit` uses, a different domain.

# The gate reads the extension, so a native-library spec is denied before it is
# loaded and no real library need exist. Counterfactual: without the derived
# requirement `import` declares only `:fs`, so `:deny |:ffi|` does not stop it
# and the fiber ends :error on the missing file rather than :paused on a denial.
(let [f (fiber/new (fn [] (import-file "nonexistent.so")) |:ffi :error|
                   :deny |:ffi|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "loading a cdylib under :deny |:ffi| is denied")
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "the denial is a capability denial")
    (assert ((get v :denied) :ffi)
            "the denial names :ffi, derived from the .so path")))

# A .lisp module is not gated by :ffi: the requirement is the operation, not the
# act of importing. Denying :ffi leaves a source import working.
(with-temp-dir dir
               (let [mod (path/join dir "m.lisp")]
                 (file/write mod "42")
                 (let [f (fiber/new (fn [] (import-file mod)) |:ffi :error :fs|
                                    :deny |:ffi|)]
                   (assert (= 42 (fiber/resume f))
                           "a .lisp import is not blocked by :deny |:ffi|")
                   (assert (= (fiber/status f) :dead)
                           "the source import ran to completion"))))

(println "caps-import-ffi: OK")
