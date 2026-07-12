(elle/epoch 12)
# Regression fixture: a top-level `protect` must evaluate at import time.
#
# `protect` expands (prelude.lisp) to a fiber/new + fiber/resume. When this
# module is imported (its forms run nested inside the caller's fiber), that
# resume returns the VM-internal SIG_SWITCH trampoline signal. The import
# executor must DRAIN SIG_SWITCH (like the root dispatch and `eval` do); if it
# leaks, the import fails with "unexpected signal 0x2000".
#
# `_p` is `[dead? value]` — `(first _p)` is true iff the protected body ran to
# completion during import.
(def _p (protect 1))

(fn [] {:ok (fn [] (first _p))})
