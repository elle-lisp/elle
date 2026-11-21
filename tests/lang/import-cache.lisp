(elle/epoch 18)
# audited: 2026-11-21
# The default loader keeps each file's value under its path and text, so every import of one text answers one value.
# docs/modules.md

(defn request [spec &opt dir]
  "The request import hands its loader for SPEC written in DIR."
  {:spec spec :dir dir})

# ── One text, one value ────────────────────────────────────────────────

(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] {:x 1})\n")
    (assert (identical? (import p) (import p))
            "two imports of one file answer one value")
    (assert (identical? (import p) (import/load (request p)))
            "import and a call of import/load share the kept value")
    (assert (not (identical? (import/load-file p) (import/load-file p)))
            "a direct load-file keeps nothing")))

# The key is the resolved path. The counter-factual keyed the spec, and a
# relative spec and an absolute one built two values.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] {:x 1})\n")
    (assert (identical? (import/load (request "./m" dir))
                        (import/load (request p)))
            "two specs that name one file answer one value")))

# The counter-factual keyed the text alone, and two files that hold one text
# answered one value.
(with-temp-dir dir
  (let [p (path/join dir "a.lisp")
        q (path/join dir "b.lisp")]
    (file/write p "(fn [] {:x 1})\n")
    (file/write q "(fn [] {:x 1})\n")
    (assert (not (identical? (import p) (import q)))
            "two files that hold one text answer two values")))

# ── An edit loads again ────────────────────────────────────────────────

(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] :before)\n")
    (let [before (import p)]
      (assert (= :before (before)) "the first text loads")
      (file/write p "(fn [] :after)\n")
      (let [after (import p)]
        (assert (= :after (after)) "an edited file loads again")
        (assert (identical? after (import p))
                "and the edited text's value is the one kept")))))

# ── A hit still reads the file ─────────────────────────────────────────

# The counter-factual answered a kept value before it read anything, so a
# fiber that withholds :fs imported a file it could not read.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] {:x 1})\n")
    (import/load (request p))
    (let [f (fiber/new (fn [] (import/load (request p))) |:fs :error|
                       :deny |:fs|)]
      (fiber/resume f)
      (assert (fiber/denied? f)
              "a fiber that withholds :fs cannot import a kept file"))))

# ── The first load's bindings decide ───────────────────────────────────

(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(*loader*)\n")
    (assert (= :first (parameterize ((*loader* :first))
                        (import/load (request p))))
            "the form reads the bindings where the first load runs")
    (assert (= :first (parameterize ((*loader* :second))
                        (import/load (request p))))
            "a later import answers the value the first load built")))

# ── Two loads at once ──────────────────────────────────────────────────

# Both imports read the file before either has loaded it, so each evaluates
# the form. The counter-factual kept the value of the last to finish, and the
# first answered a value that no later import answers.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] {:x 1})\n")
    (let [a (ev/spawn (fn [] (import p)))
          b (ev/spawn (fn [] (import p)))
          va (ev/join a)
          vb (ev/join b)]
      (assert (identical? va vb) "two loads at once answer one value")
      (assert (identical? va (import p)) "and it is the value kept"))))

# ── A failed load keeps nothing ────────────────────────────────────────

# The counter-factual kept a mark for the path before the form ran, and the
# second import answered the mark.

(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] (undefined-name))\n")
    (let [[ok? _] (protect (import p))]
      (assert (not ok?) "a form that does not compile fails the load"))
    (let [[ok? _] (protect (import p))]
      (assert (not ok?) "and fails it again, since no value was kept"))))
