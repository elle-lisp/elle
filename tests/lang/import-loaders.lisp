(elle/epoch 14)
# audited: 2026-10-06
# The three loaders each do one thing, and only the plugin loader needs :ffi.
# docs/modules.md

# ── import/load-file takes a path as slurp does ─────────────────────────

# The runner starts in the repository root.
(let [m ((import/load-file "tests/modules/test.lisp"))]
  (assert (= 42 m:test-var)
          "a relative path resolves against the working directory"))

(with-temp-dir dir
               (let [p (path/join dir "m.lisp")]
                 (file/write p "(+ 40 2)")
                 (assert (= 42 (import/load-file p))
                         "a source file's last expression is the value")))

# A file that is not UTF-8 is not source, and load-file does not guess that it
# is a library. The counter-factual hands it to the plugin loader.
(with-temp-dir dir
               (let [p (path/join dir "binary.lisp")
                     port (port/open p :write)]
                 (port/write port (bytes 255 254 0))
                 (port/close port)
                 (let [[ok? err] (protect (import/load-file p))]
                   (assert (not ok?) "a file that is not UTF-8 does not load")
                   (assert (= :io-error (get err :error)) "it fails as a read")
                   (assert (not (string/contains? (get err :message) "plugin"))
                           "and nothing tried it as a plugin"))))

# ── Only the plugin loader needs :ffi ───────────────────────────────────

# The gate refuses the call before the loader reads the path, so no library
# need exist.
(defn denied-primitive [thunk]
  "The primitive a fiber denied :ffi was refused, or nil when nothing was."
  (let [f (fiber/new thunk |:ffi :error| :deny |:ffi|)]
    (fiber/resume f)
    (when (= :paused (fiber/status f)) (get (fiber/value f) :primitive))))

(assert (= "import/load-plugin"
           (denied-primitive (fn [] (import/load-plugin "nonexistent.so"))))
        "import/load-plugin declares :ffi")
(assert (= "import/load-plugin"
           (denied-primitive (fn [] (import-file "nonexistent.so"))))
        "import-file hands a library name to the plugin loader")
(let [name "nonexistent.so"]
  (assert (= "import/load-plugin" (denied-primitive (fn [] (import-file name))))
          "and picks the loader for a computed name when the form runs"))
(with-temp-dir dir
               (let [p (path/join dir "m.lisp")]
                 (file/write p "42")
                 (let [f (fiber/new (fn [] (import/load-file p))
                                    |:ffi :error :fs| :deny |:ffi|)]
                   (assert (= 42 (fiber/resume f))
                           "import/load-file needs no :ffi"))))

# ── import/load-syntax takes one form ───────────────────────────────────

(assert (= 3 (import/load-syntax '(+ 1 2))) "a quoted form loads as a module")
(assert (= 42 (import/load-syntax (read "(+ 40 2)")))
        "so does a form read from text")

# ── A cycle reports at run time ─────────────────────────────────────────

# cycle-a and cycle-b import each other at top level through the import macro.
# Compiling a file compiles none of its imports, so nothing recurses at compile
# time, and the loader names every file in the cycle, from the one it reached
# twice.
(let [[ok? err] (protect (import "../modules/cycle-a"))
      msg (get err :message)]
  (assert (not ok?) "a top-level import cycle fails")
  (assert (string/contains? msg "circular dependency")
          "the loader names a cycle")
  (assert (string/contains? msg "cycle-a.lisp -> ")
          "the cycle starts at cycle-a")
  (assert (string/contains? msg "cycle-b.lisp -> ") "and passes through cycle-b"))
