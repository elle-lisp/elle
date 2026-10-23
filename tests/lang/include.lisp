(elle/epoch 15)
# audited: 2026-10-21
# include and include-file splice a file's macros and definitions into the including file.
# docs/modules.md

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

# ── An included form names the including file's definitions ─────────────────

(include-file "include/back.lisp")

(defn includer-answer []
  42)
(def includer-base 100)

# The forms above reach the included file only one way: the including file
# names what the included one defines. A splice that kept the included forms
# out of the including file's scope passes those, and rejects these two as
# undefined variables.
(assert (= (ask-includer) 42)
        "include-file: an included function calls the including file's function")
(assert (= (includer-base-plus 1) 101)
        "include-file: an included function reads the including file's def")
