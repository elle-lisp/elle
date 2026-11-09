(elle/epoch 15)
# audited: 2026-11-09
# The loaders are functions: load-file reads, load-source reads one form, load-syntax evaluates it under the module deny set.
# docs/modules.md

(def here (path/parent (get (meta/location) :file)))

(defn message-of [thunk]
  "The message of the error thunk raises, or nil when it raises none."
  (let [[ok? err] (protect (thunk))]
    (when (not ok?) (get err :message))))

# ── import/load-file takes a path as file/read does ────────────────────

# The runner starts in the repository root.
(let [m ((import/load-file "tests/modules/test.lisp"))]
  (assert (= 42 m:test-var)
          "a relative path resolves against the working directory"))

(with-temp-dir dir
               (let [p (path/join dir "m.lisp")]
                 (file/write p "(+ 40 2)")
                 (assert (= 42 (import/load-file p))
                         "the file's one form is the value")))

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

(let [[ok? err] (protect (import/load-file "tests/modules/no-such-module.lisp"))]
  (assert (and (not ok?) (= :io-error (get err :error)))
          "a path that names no file fails as a read"))

# ── import/load-source reads one form ───────────────────────────────────

(assert (= 1 ((import/load-source "<t>" "(fn [] 1)")))
        "a form read from text loads as a module")
(assert (= 3 (import/load-source "<t>" "(elle/epoch 15)\n(+ 1 2)"))
        "the epoch declaration is not a form")
# Epoch 10 renamed car to first. The counter-factual compiles the form as
# written, and car names nothing.
(assert (= 7 (import/load-source "<t>" "(elle/epoch 9)\n(car (list 7 8))"))
        "the form migrates from its declared epoch")

(let [[ok? err] (protect (import/load-source "<t>" "(def a 1) (fn [] a)"))]
  (assert (not ok?) "two forms do not load")
  (assert (= :module-error (get err :error)) "the loader refuses them"))
(let [[ok? err] (protect (import/load-source "<t>" "(elle/epoch 15)\n"))]
  (assert (not ok?) "no form does not load")
  (assert (= :module-error (get err :error)) "the loader refuses it"))

# The counter-factual compiled the form as a datum, and the message lost the
# name and the line.
(let [msg (message-of (fn []
                        (import/load-source "<named>"
                        "\n(fn [] (undefined-name))")))]
  (assert (string/contains? msg "<named>:2:")
          "a compile error names the source and the line"))

# ── import/load-syntax evaluates one form as a module ──────────────────

(assert (= 3 (import/load-syntax '(+ 1 2))) "a datum loads as a module")
(assert (= 42 (import/load-syntax (read "(+ 40 2)"))) "so does a read form")
(assert (= 42 (import/load-syntax (first (compile/read-forms "(+ 40 2)" "<t>"))))
        "so does a syntax object")

# The form withholds the effect capabilities, :ambient and :import, and the
# error names what the form called. The counter-factual reported an unexpected
# signal, which names nothing.
(assert (string/contains? (message-of (fn []
                                        (import/load-syntax '(sys/env "HOME"))))
                          "sys/env") "an ambient read in the form is refused")
(assert (string/contains? (message-of (fn []
                                        (import/load-syntax '(file/read "tests/modules/test.lisp"))))
                          "file/read") "a file read in the form is refused")
(assert (string/contains? (message-of (fn []
                                        (import/load-syntax '((import "std/base64")))))
                          ":import") "a form cannot import")
(assert (string/contains? (message-of (fn []
                                        (import/load-file "tests/modules/self-import.lisp")))
                          ":import")
        "so a module cannot import itself while it loads")

(let [m (import/load-syntax '(fn [] (import "std/base64")))]
  (assert (closure? m) "a form may hold an import inside a function"))

(let [[ok? err] (protect (import/load-syntax '(error {:error :raised-in-form
                         :message "raised"})))]
  (assert (not ok?) "a raise in the form fails the load")
  (assert (= :raised-in-form (get err :error)) "with the error the form raised"))

(let [m ((import/load-file "tests/modules/protect-toplevel.lisp"))]
  (assert (= true (m:ok)) "a protect in the form runs while the form loads"))

# ── The loader import uses picks by name ───────────────────────────────

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
(with-temp-dir dir
               (let [lib (path/join dir "fake.so")
                     src (path/join dir "m.lisp")]
                 (file/write lib "")
                 (file/write src "42")
                 (assert (= "import/load-plugin"
                            (denied-primitive (fn []
                              (import/load {:spec lib :dir nil}))))
                         "import/load hands a library name to the plugin loader")
                 (assert (= 42 (import/load {:spec "./m" :dir dir}))
                         "and every other name to load-file, resolved against :dir")
                 (assert (nil? (denied-primitive (fn []
                                 (import/load {:spec src :dir nil}))))
                         "which needs no :ffi")))

# ── import-file is gone, and an older epoch reads it as import ─────────

(let [[ok? _] (protect (eval '(import-file "tests/modules/test.lisp")))]
  (assert (not ok?) "import-file names nothing"))

# The file the thunk runs imports through this file's handler.
(let [unit (compile/whole-module "(elle/epoch 15)\n((import-file \"std/base64\"))"
                                 "<epoch-15>")
      b64 ((get (get unit 0) 1))]
  (assert (= "aGk=" (b64:encode "hi")) "epoch 15's import-file imports"))
