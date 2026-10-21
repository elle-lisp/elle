(elle/epoch 15)
# audited: 2026-10-21
# include and include-file splice a file's macros and definitions into the including file.
# docs/include.md
#

# ── include-file with relative path ──────────────────────────────────────────

(include-file "inclib.lisp")

# macro defined in included file is available
(assert (= (double-it 5) 10) "include-file: macro from included file")

# function defined in included file is available
(assert (= (triple 4) 12) "include-file: function from included file")

# ── include resolves its spec with import/resolve ───────────────────────────

# A ../ spec names a file relative to this one, through the same rules import
# follows. The counter-factual resolves it against the working directory, where
# it names nothing.
(include "../modules/incspec")

(assert (= (quadruple-it 7) 28) "include: macro from a file a spec names")

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
