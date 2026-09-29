(elle/epoch 12)
# audited: 2026-09-29
# include and include-file splice a file's macros and definitions into the including file.
# docs/modules.md
#

# ── include-file with relative path ──────────────────────────────────────────

(include-file "inclib.lisp")

# macro defined in included file is available
(assert (= (double-it 5) 10) "include-file: macro from included file")

# function defined in included file is available
(assert (= (triple 4) 12) "include-file: function from included file")

# ── include with search-path resolution ──────────────────────────────────────

(include "tests/lang/inclib")

# same macro available again (re-included via search path)
(assert (= (double-it 7) 14) "include: macro via search path")
