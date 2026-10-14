(elle/epoch 14)
# audited: 2026-10-06
# include and include-file splice a file's macros and definitions into the including file.
# docs/modules.md
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
