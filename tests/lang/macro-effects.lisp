(elle/epoch 15)
# audited: 2026-10-22
# A macro expands without the effect capabilities, on a fiber that withholds nothing; the code it expands to runs with them.
#
# docs/signals/capabilities.md
# docs/macros.md
#
# Each compile below builds its source as text and reads it at run time. A
# quoted form would not do: an explicit quote is macro-expanded at the outer
# compile, which runs outside every sandbox.

# Source text for `(defmacro m [] (file/read PATH))`.
(defn reader-macro [path]
  (string/join ["(defmacro m [] (file/read \"" path "\"))"] ""))

# Source text for MACRO and then a call of it.
(defn forms [macro]
  (string/join [macro " (m)"] ""))

# The same two forms as one, for a compile that takes a single form.
(defn program [macro]
  (string/join ["(begin " (forms macro) ")"] ""))

# Run `body` on this fiber, which withholds nothing, and answer the error it
# raised.
(defn unrestricted [body what]
  (let [[ok? err] (protect (body))]
    (assert (not ok?) (string/join [what ": the compile fails"] ""))
    err))

# Assert that `v` is the error a failed compile raised: `:eval-error` from
# `eval` and the loaders, `:compile-error` from `compile/analyze`.
(defn assert-compile-error [v what]
  (assert (struct? v) (string/join [what ": the compile answers no value"] ""))
  (assert (contains? |:eval-error :compile-error| (get v :error))
          (string/join [what ": the compile fails, and no denial escapes it"] "")))

(defn assert-denied [v primitive what]
  (assert-compile-error v what)
  (let [msg (get v :message)]
    (assert (string/contains? msg "macro 'm'")
            (string/join [what ": the error names the macro: " msg] ""))
    (assert (string/contains? msg primitive)
            (string/join [what ": the error names " primitive ": " msg] ""))))

(with-temp-dir root (def secret (path/join root "secret"))
               (file/write secret "3311")

               # ── What every expansion withholds ──────────────────────────────────

               # This fiber withholds nothing, and still no macro reads a file. The
               # counter-factual would expand each macro with every capability and answer
               # "3311".
               (def read-secret (reader-macro secret))
               (assert-denied (unrestricted (fn []
                                (eval (read (program read-secret)))) "eval")
                              "file/read" "eval")
               (assert-denied (unrestricted (fn []
                                (compile/analyze (forms read-secret)))
                              "compile/analyze") "file/read" "compile/analyze")
               (assert-denied (unrestricted (fn []
                                (import/load-syntax (read (program read-secret))))
                              "import/load-syntax") "file/read"
                              "import/load-syntax")

               # `ffi/native` answers a handle to the process itself, so a macro that
               # called it would load nothing and still reach outside the unit.
               (def native (path/join root "native.lisp"))
               (file/write native
                           "(defmacro m [] (begin (ffi/native nil) 7))\n(m)\n")
               (assert-denied (unrestricted (fn [] (import-file native))
                              "import-file") "ffi/native" "import-file")

               # ── Code a compile produced ─────────────────────────────────────────

               # The expansion ends before its unit runs, and the unit runs with the
               # capabilities of its fiber. The counter-factual would leave the effect
               # bits withheld past the expansion, and the read would be denied.
               (def after
                 (string/join ["(begin (defmacro w [] 0) (w) (file/read \""
                               secret "\"))"] ""))
               (assert (= "3311" (eval (read after)))
                       "run-time code reads the file"))

(println "macro-effects: OK")
