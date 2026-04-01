# include / include-file tests

# ── include-file with relative path ──────────────────────────────────────────

(include-file "inclib.lisp")

# macro defined in included file is available
(assert (= (double-it 5) 10) "include-file: macro from included file")

# function defined in included file is available
(assert (= (triple 4) 12) "include-file: function from included file")

# ── include with search-path resolution ──────────────────────────────────────

(include "tests/elle/inclib")

# same macro available again (re-included via search path)
(assert (= (double-it 7) 14) "include: macro via search path")
