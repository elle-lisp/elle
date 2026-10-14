(elle/epoch 15)
# audited: 2026-10-14
# A macro body reads no state outside its unit: an ambient read halts the expansion past every handler, and run-time code is free.
# docs/macros.md

# The message of the error a compile of `form` raised.
(defn compile-error [form what]
  (let [[ok? err] (protect (eval form))]
    (assert (not ok?) (string/join [what ": the compile fails"] ""))
    (get err :message)))

# Assert that compiling `form` fails, naming `name` and `primitive`.
(defn assert-refused [form name primitive what]
  (let [msg (compile-error form what)]
    (assert (string/contains? msg (string/join ["macro '" name "'"] ""))
            (string/join [what ": the error names the macro: " msg] ""))
    (assert (string/contains? msg primitive)
            (string/join [what ": the error names " primitive ": " msg] ""))))

# ── An ambient read halts the expansion ───────────────────────────────

# The counter-factual for each: the macro read the environment, the pid or a
# clock, and expanded to what it read.
(assert-refused '(begin
                   (defmacro home []
                     (sys/env "HOME"))
                   (home)) "home" "sys/env" "sys/env")
(assert-refused '(begin
                   (defmacro now []
                     (clock/monotonic))
                   (now)) "now" "clock/monotonic" "clock/monotonic")

# The refusal is a halt, so `protect` inside the body does not catch it, and
# the body never reads the error in place of the environment.
(assert-refused '(begin
                   (defmacro home []
                     (protect (sys/env "HOME")))
                   (home)) "home" "sys/env" "sys/env under protect")

# The halt carries :terminal, so a fiber whose mask names :halt does not catch
# it either. The counter-factual: a plain halt, which that mask absorbs, and
# the macro expanded to the refusal's payload.
(assert-refused '(begin
                   (defmacro pid []
                     (fiber/resume (fiber/new (fn [] (sys/pid)) |:halt :error|)))
                   (pid)) "pid" "sys/pid"
                "sys/pid in a fiber that catches :halt")

# A `begin-for-syntax` body runs at expansion as a transformer does.
(let [msg (compile-error '(begin-for-syntax (def home (sys/env "HOME")))
                         "begin-for-syntax")]
  (assert (string/contains? msg "sys/env")
          (string/join ["begin-for-syntax: the error names sys/env: " msg] "")))

# ── What a macro body may still do ────────────────────────────────────

# Pure computation over the unit's inputs, and over the primitives the compile
# binds, which are an input too. The counter-factual marked too wide a set.
(defmacro shaped [& names]
  (let [joined (string/join (map (fn [n] (string (syntax->datum n))) names) "-")
        tmp (gensym "t")]
    `(let [,tmp ,joined]
       ,tmp)))
(assert (= (shaped a b c) "a-b-c") "gensym and string operations expand")

(defmacro primitive-count []
  (length (vm/list-primitives)))
(assert (= (primitive-count) (length (vm/list-primitives)))
        "a macro reads the primitives the compile binds")

# ── Code the expansion produced ───────────────────────────────────────

# The refusal belongs to the expansion. Code a macro expands to reads the
# environment when it runs, and so does code after a refused compile. The
# counter-factual left the refusal standing past the expansion.
(defmacro pid-form []
  '(sys/pid))
(assert (= (pid-form) (sys/pid)) "an expansion may call an ambient primitive")

(compile-error '(begin
                  (defmacro home []
                    (sys/env "HOME"))
                  (home)) "refused")
(assert (integer? (sys/pid)) "a refused compile leaves run time free")
(assert (struct? (sys/env)) "and the environment readable")

# ── What a reader of the primitive learns ─────────────────────────────

# The counter-factual: the mark was invisible, and a reader learned of it
# from a failed compile.
(assert (get (vm/primitive-meta "sys/env") :ambient) "sys/env carries the mark")
(assert (not (get (vm/primitive-meta "string/join") :ambient))
        "string/join carries none")
(assert (string/contains? (doc sys/env) "expansion")
        "the docstring says an expansion refuses it")

(println "macro-purity: OK")
