(elle/epoch 15)
# audited: 2026-10-22
# A macro expands without what the compiling fiber withholds, and a denied primitive fails the compile.
#
# docs/signals/capabilities.md
# docs/macros.md
#
# Each compile below builds its source as text and reads it at run time. A
# quoted form would not do: an explicit quote is macro-expanded at the outer
# compile, which runs outside every sandbox.

(signal :caps-macro/beat)

# Source text for `(defmacro m [] (emit :caps-macro/beat 1))`. The bit is no
# effect capability, so only a fiber's :deny withholds it.
(def beat-macro "(defmacro m [] (emit :caps-macro/beat 1))")

# Source text for MACRO and then a call of it.
(defn forms [macro]
  (string/join [macro " (m)"] ""))

# The same two forms as one, for a compile that takes a single form.
(defn program [macro]
  (string/join ["(begin " (forms macro) ")"] ""))

# Source text for `(include-file PATH)`.
(defn include-program [path]
  (string/join ["(include-file \"" path "\")"] ""))

# Run `body` in a fiber that withholds `deny`, and answer what it returns or
# signals. A caught error leaves the fiber :paused, as an escaped denial
# does, so the value tells the two apart. The mask catches the denied bits
# too, and a denial that escaped the compile comes back as its payload.
(defn sandboxed [deny body]
  (fiber/resume (fiber/new body |:caps-macro/beat :fs :error| :deny deny)))

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

(with-temp-dir root
               # ── What the compiling fiber withholds ──────────────────────────────

               # Each compile a fiber starts expands without the bits that fiber withholds.
               # The counter-factual: the macro would emit, and the compile would report a
               # suspension that named no primitive.
               (def deny-beat |:caps-macro/beat|)

               # `eval` and `compile/analyze` expand on the calling fiber.
               (assert-denied (sandboxed deny-beat
                              (fn [] (eval (read (program beat-macro)))))
                              "fiber/emit" "eval")
               (assert-denied (sandboxed deny-beat
                              (fn [] (compile/analyze (forms beat-macro))))
                              "fiber/emit" "compile/analyze")

               # `import/load-syntax` and `import-file` compile on the instance's macro VM.
               (assert-denied (sandboxed deny-beat
                              (fn []
                                (import/load-syntax (read (program beat-macro)))))
                              "fiber/emit" "import/load-syntax")
               (def module (path/join root "m.lisp"))
               (file/write module (string/join [(forms beat-macro) "\n"] ""))
               (assert-denied (sandboxed deny-beat (fn [] (import-file module)))
                              "fiber/emit" "import-file")

               # ── An include ──────────────────────────────────────────────────────

               # An include is an input of the compile, so it reads under the fiber's own
               # capabilities. The counter-factual for the denial would splice the file in
               # and answer 42.
               (def included (path/join root "inc.lisp"))
               (file/write included "42\n")
               (defn load-include []
                 (import/load-syntax (read (include-program included))))
               (let [v (sandboxed |:fs| load-include)]
                 (assert-compile-error v "include-file under :deny |:fs|")
                 (assert (string/contains? (get v :message) "include")
                         "the error names the include"))
               (assert (= 42 (load-include))
                       "the denial does not outlast its compile, and an unrestricted one includes"))

(println "caps-macro: OK")
