# tests/elle/errors.lisp
# Smoke-tests that specific error keywords are produced.
# Each assert-err-kind call verifies the :error field keyword.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ── argument-error ───────────────────────────────────────────────────────────
(assert-err-kind (fn [] (pop @[])) :argument-error "pop empty @array")
(assert-err-kind (fn [] (range 0 10 0)) :argument-error "range zero step")

# ── parse-error ──────────────────────────────────────────────────────────────
(assert-err-kind (fn [] (float "nope")) :parse-error "float parse bad input")
(assert-err-kind (fn [] (integer "nope")) :parse-error "integer parse bad input")

# ── encoding-error ───────────────────────────────────────────────────────────
(assert-err-kind (fn [] (string (bytes 0xff 0xfe))) :encoding-error "bytes->string bad utf8")

# ── io-error (slurp) ─────────────────────────────────────────────────────────
(assert-err-kind (fn [] (slurp "/no/such/file/at/all")) :io-error "slurp missing file")

# ── io-error extra :path field ───────────────────────────────────────────────
(let (([ok? err] (protect (slurp "/no/such/file/at/all"))))
  (assert-false ok? "slurp should error")
  (assert-eq (get err :error) :io-error "slurp error kind is :io-error")
  (assert-true (string? (get err :path)) "slurp error has :path field"))

# ── state-error (fiber) ──────────────────────────────────────────────────────
(let ((f (fiber/new (fn [] 42) 0)))
  (fiber/resume f)
  (assert-err-kind (fn [] (fiber/resume f)) :state-error "resume completed fiber"))

# ── state-error (chan) ────────────────────────────────────────────────────────
(let (([tx _] (chan)))
  (chan/close tx)
  (assert-err-kind (fn [] (chan/clone tx)) :state-error "clone closed sender"))

# ── signal-error ─────────────────────────────────────────────────────────────
(assert-err-kind
  (fn [] (fiber/new (fn [] 42) :not-a-signal-keyword))
  :signal-error
  "fiber/new unknown signal keyword")

# ── stack-overflow ────────────────────────────────────────────────────────────
# stack-overflow is hard to reliably trigger without killing the test process;
# leave this commented out for now and rely on the JIT unit tests.
# (assert-err-kind (fn [] (let loop () (loop))) :stack-overflow "infinite recursion")

# ── internal-error (gensym without symbol table) — not easily testable in Elle
# Skip: requires running without symbol table context.
