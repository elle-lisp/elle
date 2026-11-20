(elle/epoch 15)
# audited: 2026-10-26
# A module whose form runs a protect when it loads, for tests/lang/modules.lisp.
# docs/modules.md
#
# `protect` expands to fiber/new and fiber/resume, and the resume returns the
# VM's fiber-switch trampoline signal to the code that runs the form. The
# loader drains it as the root dispatch does. The counter-factual leaks it, and
# the load fails with an unexpected signal.
#
# `_p` is `[dead? value]`, so `(first _p)` is true only when the protected
# body ran to its end while the form loaded.

(let [_p (protect 1)]
  (fn [] {:ok (fn [] (first _p))}))
