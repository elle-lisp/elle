(elle/epoch 14)
# audited: 2026-10-06
# Loading a native library under :deny |:ffi| is refused, and loading Elle source is not.
# docs/signals/capabilities.md

# `import-file` hands a library name to `import/load-plugin`, which declares
# :ffi because a library's `elle_plugin_init` is foreign code. The gate refuses
# the call before the loader reads the path, so no real library need exist.
# The counter-factual: a plugin loader that declares only :fs is not stopped by
# `:deny |:ffi|`, and the fiber ends :error on the missing file rather than
# :paused on a denial.
(let [f (fiber/new (fn [] (import-file "nonexistent.so")) |:ffi :error|
                   :deny |:ffi|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "loading a cdylib under :deny |:ffi| is denied")
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "the denial is a capability denial")
    (assert ((get v :denied) :ffi) "the denial names :ffi")))

# A .lisp module loads through `import/load-file`, which declares :fs and no
# :ffi, so denying :ffi leaves a source load working.
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
