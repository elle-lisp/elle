(elle/epoch 15)
# audited: 2026-10-26
# A file loaded again compiles and runs again: what its text, its includes and the caller's capabilities make of it now.
# docs/modules.md

# Each case loads one file more than once. The counter-factual for each kept
# the first load's code under the file's path alone, and ran it for every
# later load.

# ── Every load runs the file ─────────────────────────────────────────────

# The state lives in the function each load answers, so each load's calls
# count on their own.
(with-temp-dir dir
               (let [p (path/join dir "state.lisp")]
                 (file/write p
                             "(fn [] (def items @[]) (def table @{}) (var n 0) (fn [x] (push items x) (put table x true) (assign n (+ n 1)) [(length items) (length table) n]))\n")
                 (let [a ((import/load-file p))
                       b ((import/load-file p))]
                   (a :one)
                   (a :two)
                   (assert (= [1 1 1] (b :one))
                           "a second load builds its own state"))))

# ── The text the file holds now ──────────────────────────────────────────

(with-temp-dir dir
               (let [p (path/join dir "edited.lisp")]
                 (file/write p "(fn [] :first)\n")
                 (assert (= :first ((import/load-file p))) "the first text runs")
                 (file/write p "(fn [] :second)\n")
                 (assert (= :second ((import/load-file p)))
                         "a load runs the text the file holds now")
                 (file/write p "(fn [] :first)\n")
                 (assert (= :first ((import/load-file p)))
                         "and the earlier text again when it comes back")))

# ── Its includes ─────────────────────────────────────────────────────────

# An edited include changes what the including file compiles to.
(with-temp-dir dir
               (let [main (path/join dir "main.lisp")
                     inc (path/join dir "inc.lisp")]
                 (file/write inc "(defn answer [] 1)\n")
                 (file/write main
                             "(begin (include-file \"inc.lisp\") (answer))\n")
                 (assert (= 1 (import/load-file main)) "the include's text runs")
                 (file/write inc "(defn answer [] 2)\n")
                 (assert (= 2 (import/load-file main))
                         "the edited include's text runs")))

# A file that now comes first in the include's resolution replaces the file it
# named. The counter-factual read again only the file the first compile
# included, found it unchanged, and answered 1.
(with-temp-dir dir
               (let [main (path/join dir "main.lisp")]
                 (file/write (path/join dir "inc") "(defn answer [] 1)\n")
                 (file/write main "(begin (include \"./inc\") (answer))\n")
                 (assert (= 1 (import/load-file main))
                         "./inc names the bare file")
                 (file/write (path/join dir "inc.lisp") "(defn answer [] 2)\n")
                 (assert (= 2 (import/load-file main))
                         "./inc now names inc.lisp, which runs")))

# ── The capabilities of the caller ───────────────────────────────────────

# The macro expands to :beat when its fiber may emit the user signal, and to
# :denied when the fiber that started the compile withholds it. The mask names
# no :io, so the loader's read reaches the scheduler.
(signal :import-again/beat)

(defn beat-load [p]
  "Load p on a fiber that withholds :import-again/beat."
  (fiber/resume (fiber/new (fn [] (import/load-file p))
                           |:import-again/beat :error|
                           :deny |:import-again/beat|)))

(with-temp-dir dir
               (let [p (path/join dir "beat.lisp")]
                 (file/write p
                             "(begin (defmacro m []\n  (let [v (fiber/resume (fiber/new (fn [] (emit :import-again/beat 1) 5) |:import-again/beat|))]\n    (if (= v 1) :beat :denied))) (m))\n")
                 (assert (= :beat (import/load-file p))
                         "an unrestricted compile may emit")
                 (assert (= :denied (beat-load p))
                         "a compile for a fiber that withholds the bit may not")
                 (assert (= :beat (import/load-file p))
                         "and an unrestricted compile may again")))

(println "import-again: OK")
