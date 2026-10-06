(elle/epoch 14)
# audited: 2026-10-06
# A macro expands with the capabilities the compiling fiber withholds, and a denied primitive fails that compile.
# docs/signals/capabilities.md
# docs/macros.md

# Each sandbox below builds its source as text and reads it inside the fiber.
# A quoted form would not do: an explicit quote is macro-expanded at the outer
# compile (#1229), which runs outside every sandbox.

(def root (file/mktempdir))
(def secret (path/join root "secret"))
(file/write secret "3311")

# Source text for `(defmacro NAME [] (file/read SECRET))`.
(defn reader-macro [name]
  (string/join ["(defmacro " name " [] (file/read \"" secret "\"))"] ""))

# Run `body` in a fiber that withholds `deny`, and answer what it returns or
# signals. A caught error leaves the fiber :paused, as an escaped denial
# does, so the value tells the two apart: the mask catches the denied bits
# too, and a denial that escaped the compile comes back as its payload.
(defn sandboxed [deny body]
  (fiber/resume (fiber/new body |:fs :ffi :error| :deny deny)))

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

# ── Every compile a fiber starts ──────────────────────────────────────

# `eval` expands on the calling fiber. The counter-factual reported an
# unexpected signal from the transformer and named neither the macro nor the
# primitive.
(assert-denied (sandboxed |:fs|
                          (fn []
                            (eval (read (string/join ["(begin "
                                        (reader-macro "m") " (m))"] "")))))
               "file/read" "eval")

# `import/load-syntax` compiles on the instance's macro VM. The counter-factual
# ran the transformer there with every capability and answered "3311".
(assert-denied (sandboxed |:fs|
                          (fn []
                            (import/load-syntax (read (string/join ["(begin "
                            (reader-macro "m") " (m))"] ""))))) "file/read"
               "import/load-syntax")

# A file loaded through `import-file` compiles on the macro VM as well. The
# loader needs :fs, so this fiber withholds :ffi, and the module's macro calls
# `ffi/native`, which answers a handle to the process itself when it runs. The
# counter-factual expanded the macro and answered 7.
(def module (path/join root "m.lisp"))
(file/write module "(defmacro m [] (begin (ffi/native nil) 7))\n(m)\n")
(assert-denied (sandboxed |:ffi| (fn [] (import-file module))) "ffi/native"
               "import-file")

# `compile/analyze` expands on the calling fiber, as `eval` does.
(assert-denied (sandboxed |:fs|
                          (fn []
                            (compile/analyze (string/join [(reader-macro "m")
                            " (m)"] "")))) "file/read" "compile/analyze")

# ── An include reads under the same rule ──────────────────────────────

# The counter-factual spliced the file in and answered 42.
(def included (path/join root "inc.lisp"))
(file/write included "42\n")
(let [v (sandboxed |:fs|
                   (fn []
                     (import/load-syntax (read (string/join ["(include-file \""
                     included "\")"] "")))))]
  (assert-compile-error v "include-file under :deny |:fs|")
  (assert (string/contains? (get v :message) "include")
          "the error names the include"))

# ── A compile no fiber restricts ──────────────────────────────────────

(assert (= "3311"
           (eval (read (string/join ["(begin " (reader-macro "u") " (u))"] ""))))
        "eval of an unrestricted fiber runs a macro that reads a file")

# The denials above ran on the instance's macro VM, and what they withheld
# does not outlast them.
(assert (= "3311"
           (import/load-syntax (read (string/join ["(begin " (reader-macro "v")
                                     " (v))"] ""))))
        "a later compile no fiber restricts reads the file")
(assert (= 42
           (import/load-syntax (read (string/join ["(include-file \"" included
                                     "\")"] "")))) "and includes one")

(file/delete-dir-all root)
(println "caps-macro: OK")
