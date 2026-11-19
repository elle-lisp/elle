(elle/epoch 15)
# audited: 2026-11-19
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

(defn includer-answer []
  42)
(def includer-base 100)

(include-file "include/back.lisp")

# The included functions reach the two definitions above only through the
# including file's scope. A splice that kept the included forms out of that
# scope rejects both as undefined variables.
(assert (= (ask-includer) 42)
        "include-file: an included function calls the including file's function")
(assert (= (includer-base-plus 1) 101)
        "include-file: an included function reads the including file's def")

# ── An included form sees only the definitions before the include ───────────

# A name exists from its def onward, for a form an include splices as for one
# written in its place. Up to epoch 17 a file was one letrec, so an included
# function could call a function the including file defined after it.
(with-temp-dir dir
  (file/write (path/join dir "inc.lisp") "(defn ask [] (later))\n")
  (file/write (path/join dir "main.lisp")
              "(begin (include-file \"inc.lisp\") (defn later [] 42) (ask))\n")
  (let [[ok? err] (protect (import/load-file (path/join dir "main.lisp")))]
    (assert (not ok?)
            "include-file: an included form that reads a later def fails")
    (assert (string/contains? (get err :message) "undefined variable: later")
            (string "include-file: the error names the later def: "
                    (get err :message)))))
