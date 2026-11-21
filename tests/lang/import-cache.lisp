(elle/epoch 18)
# audited: 2026-11-21
# The default loader keeps each file's compiled form: one compile serves every import of a text, and each import evaluates the form.
# docs/modules.md

(defn request [spec &opt dir]
  "The request import hands its loader for SPEC written in DIR."
  {:spec spec :dir dir})

# The compiler builds an immutable literal once for each compile, and a struct
# literal on each evaluation. So two values of this text hold one array only
# when one compiled form built both.
(def shared-text "{:k [1 2 3]}\n")

(defn constant [value]
  "The array a value of shared-text holds."
  (get value :k))

# ── Every import evaluates the form ────────────────────────────────────

# The counter-factual kept each file's value, and every import answered it.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] {:x 1})\n")
    (assert (not (%identical? (import p) (import p)))
            "two imports of one file build two values")))

# A parameter is a leaf of the purity walk, so a module's value may be one.
# The counter-factual kept the value, and both imports answered one parameter.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(make-parameter @[])\n")
    (let [a (import p)
          b (import p)]
      (push (a) :mark)
      (assert (empty? (b)) "each import builds a parameter of its own"))))

# The form answers what *loader* holds, so the binding around a direct call of
# import/load decides the value. The counter-factual kept the first value.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(*loader*)\n")
    (assert (= :first (parameterize ((*loader* :first))
                        (import/load (request p))))
            "the form reads the bindings where the first import stands")
    (assert (= :second (parameterize ((*loader* :second))
                         (import/load (request p))))
            "and a later import reads the bindings where it stands")))

(let [[ok? err] (protect (eval '(import/kept)))]
  (assert (not ok?) "import/kept names nothing")
  (assert (string/contains? (get err :message) "import/kept")
          "and the error names it"))

# ── One text, one compile ──────────────────────────────────────────────

# The counter-factual compiled the text on every import.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p shared-text)
    (assert (%identical? (constant (import p)) (constant (import p)))
            "two imports of one file share one compile")))

# The counter-factual keyed the spec, and a relative spec and an absolute one
# compiled twice.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p shared-text)
    (assert (%identical? (constant (import/load (request "./m" dir)))
                         (constant (import/load (request p))))
            "two specs that name one file share one compile")))

# A compiled form holds the location it compiled at. The counter-factual keyed
# the text alone, and the second file answered the first file's path.
(with-temp-dir dir
  (let [a (path/join dir "a.lisp")
        b (path/join dir "b.lisp")]
    (file/write a "(get (meta/location) :file)\n")
    (file/write b "(get (meta/location) :file)\n")
    (assert (= (import/resolve a) (import a)) "a file answers its own location")
    (assert (= (import/resolve b) (import b))
            "and so does a second file that holds the same text")))

# The counter-factual bound a new table in every ev/run, and the import inside
# compiled again.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p shared-text)
    (let [outer (constant (import p))]
      (assert (ev/run (fn [] (%identical? outer (constant (import p)))))
              "an ev/run inside the program shares its compiles"))))

# ── An edit compiles again ─────────────────────────────────────────────

# The first counter-factual keyed the path alone, and the edited file answered
# the old text's value. The second never replaced what it kept, so every
# import of the edited text compiled again.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "{:k [:before]}\n")
    (assert (= [:before] (constant (import p))) "the first text loads")
    (file/write p "{:k [:after]}\n")
    (assert (= [:after] (constant (import p))) "an edited file compiles again")
    (assert (%identical? (constant (import p)) (constant (import p)))
            "and the new compile is the one kept")))

# ── A text that does not compile keeps nothing ─────────────────────────

# The counter-factual kept a mark for the text before it compiled, and the
# second import ran the mark.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(fn [] (undefined-name))\n")
    (let [[ok? err] (protect (import p))
          [again? again-err] (protect (import p))]
      (assert (not ok?) "a form that does not compile fails the load")
      (assert (string/contains? (get err :message) "undefined-name")
              "and the error names what the form could not resolve")
      (assert (not again?) "a second import fails too")
      (assert (= err again-err) "with the same error"))))

# ── An import that compiles nothing loads as a first import does ───────

# The counter-factual looked the path up before it read anything, so a fiber
# that withholds :fs imported a file it could not read.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p shared-text)
    (import/load (request p))
    (let [f (fiber/new (fn [] (import/load (request p))) |:fs :error|
                       :deny |:fs|)]
      (fiber/resume f)
      (assert (fiber/denied? f)
              "a fiber that withholds :fs cannot import a kept file"))))

# The counter-factual answered a kept form's value without the walk, and
# handed the mutable array to the importer.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "(*loader*)\n")
    (assert (= :plain (parameterize ((*loader* :plain))
                        (import/load (request p))))
            "the form answers what the parameter holds")
    (let [[ok? err] (protect (parameterize ((*loader* @[1]))
                               (import/load (request p))))]
      (assert (not ok?) "a kept form that answers mutable state does not load")
      (assert (= :module-error (get err :error)) "the walk refuses its value"))))

# The form calls what *loader* holds. The counter-factual called a kept form
# on the importing fiber, which withholds nothing.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p "((*loader*))\n")
    (assert (= 1
               (parameterize ((*loader* (fn [] 1)))
                 (import/load (request p))))
            "the form calls what the parameter holds")
    (let [[ok? err] (protect (parameterize ((*loader* (fn [] (sys/env "HOME"))))
                               (import/load (request p))))]
      (assert (not ok?) "a kept form that reads ambient state does not load")
      (assert (string/contains? (get err :message) "sys/env")
              "and the refusal names the primitive"))))

# ── The other loaders keep nothing ─────────────────────────────────────

# The counter-factual kept the compiled form where import/load-source loads a
# text, so import/load-file shared it.
(with-temp-dir dir
  (let [p (path/join dir "m.lisp")]
    (file/write p shared-text)
    (assert (not (%identical? (constant (import/load-file p))
                              (constant (import/load-file p))))
            "two calls of import/load-file compile twice")))
