(elle/epoch 10)
# tests/elle/errors.lisp
# Smoke-tests that specific error keywords are produced.
# Each assert-err-kind call verifies the :error field keyword.


# ── argument-error ───────────────────────────────────────────────────────────
(let [[ok? err] (protect ((fn [] (pop @[]))))]
  (assert (not ok?) "pop empty @array")
  (assert (= (get err :error) :argument-error) "pop empty @array"))
(let [[ok? err] (protect ((fn [] (range 0 10 0))))]
  (assert (not ok?) "range zero step")
  (assert (= (get err :error) :argument-error) "range zero step"))

# ── parse-error ──────────────────────────────────────────────────────────────
(let [[ok? err] (protect ((fn [] (parse-float "nope"))))]
  (assert (not ok?) "parse-float bad input")
  (assert (= (get err :error) :parse-error) "parse-float bad input"))
(let [[ok? err] (protect ((fn [] (parse-int "nope"))))]
  (assert (not ok?) "parse-int bad input")
  (assert (= (get err :error) :parse-error) "parse-int bad input"))

# ── encoding-error ───────────────────────────────────────────────────────────
(let [[ok? err] (protect ((fn [] (string (bytes 0xff 0xfe)))))]
  (assert (not ok?) "bytes->string bad utf8")
  (assert (= (get err :error) :encoding-error) "bytes->string bad utf8"))

# ── io-error (slurp) ─────────────────────────────────────────────────────────
(let [[ok? err] (protect ((fn [] (slurp "/no/such/file/at/all"))))]
  (assert (not ok?) "slurp missing file")
  (assert (= (get err :error) :io-error) "slurp missing file"))

# ── io-error extra :path field ───────────────────────────────────────────────
(let [[ok? err] (protect (slurp "/no/such/file/at/all"))]
  (assert (not ok?) "slurp should error")
  (assert (= (get err :error) :io-error) "slurp error kind is :io-error")
  (assert (string? (get err :path)) "slurp error has :path field"))

# ── state-error (fiber) ──────────────────────────────────────────────────────
(let [f (fiber/new (fn [] 42) 0)]
  (fiber/resume f)
  (let [[ok? err] (protect ((fn [] (fiber/resume f))))]
    (assert (not ok?) "resume completed fiber")
    (assert (= (get err :error) :state-error) "resume completed fiber")))

# ── state-error (chan) ────────────────────────────────────────────────────────
(let [[tx _] (chan)]
  (chan/close tx)
  (let [[ok? err] (protect ((fn [] (chan/clone tx))))]
    (assert (not ok?) "clone closed sender")
    (assert (= (get err :error) :state-error) "clone closed sender")))

# ── signal-error ─────────────────────────────────────────────────────────────
(let [[ok? err] (protect ((fn [] (fiber/new (fn [] 42) :not-a-signal-keyword))))]
  (assert (not ok?) "fiber/new unknown signal keyword")
  (assert (= (get err :error) :signal-error) "fiber/new unknown signal keyword"))
# ── stack-overflow ────────────────────────────────────────────────────────────
# Deep non-tail recursion must produce a catchable error, not SIGABRT.
(let [[ok? err] (protect ((fn []
                            (letrec [f (fn (n)
                                         (if (= n 0)
                                           (list)
                                           (pair n (f (- n 1)))))]
                              (length (f 100000))))))]
  (assert (not ok?) "deep recursion produces error")
  (assert (= (get err :error) :stack-overflow)
          "deep recursion error kind is :stack-overflow"))
# ── internal-error (gensym without symbol table) — not easily testable in Elle
# Skip: requires running without symbol table context.
