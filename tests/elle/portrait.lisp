(elle/epoch 12)
# ── portrait library tests ──────────────────────────────────────────────

(def portrait ((import "std/portrait")))

# ── Pure function portrait ──────────────────────────────────────────────

(def src1 "
(defn add [a b] (+ a b))
(defn double [x] (add x x))
")
(def a1 (compile/analyze src1))

(def p1 (portrait:function a1 :add))
(assert (= (get p1 :name) "add") "portrait has name")
# add has SIG_ERROR (from +) — any signal is a potential suspension
(assert (not (get (get p1 :signal) :silent)) "add has SIG_ERROR from +")
(assert (empty? (get p1 :captures)) "add has no captures")
# add has SIG_ERROR so portrait considers it non-memoizable (conservative)
(assert (not (get (get p1 :composition) :memoizable))
        "add not memoizable (has SIG_ERROR)")
(assert (get (get p1 :composition) :parallelizable) "add is parallelizable")
(assert (not (get (get p1 :composition) :jit-eligible))
        "add not jit-eligible (may error)")
(assert (get (get p1 :composition) :stateless) "add is stateless")

# double calls add
(def p2 (portrait:function a1 :double))
(assert (not (empty? (get p2 :callees))) "double has callees")

# ── Rendering ───────────────────────────────────────────────────────────

(def text (portrait:render p1))
(assert (string? text) "render returns string")
(assert (> (length text) 0) "render is non-empty")
(assert (contains? text "add") "render contains function name")
# add has SIG_ERROR, so render shows "error" in signal info
(assert (contains? text "error") "render shows signal")

# ── Module portrait ─────────────────────────────────────────────────────

(def mp (portrait:module a1))
(assert (array? (get mp :pure)) "module has pure list")
# add/double have SIG_ERROR, so portrait doesn't classify them as pure
(assert (empty? (get mp :pure)) "no pure functions (SIG_ERROR from arithmetic)")

(def mod-text (portrait:render-module mp))
(assert (string? mod-text) "module render returns string")
(assert (> (length mod-text) 0) "module render is non-empty")

# ── Higher-order function ───────────────────────────────────────────────

(def src2
  "
(defn my-map [f lst]
  (if (empty? lst)
    ()
    (pair (f (first lst)) (my-map f (rest lst)))))
")
(def a2 (compile/analyze src2))
(def p3 (portrait:function a2 :my-map))

# my-map propagates parameter 0's signals
(assert (not (empty? (get (get p3 :signal) :propagates)))
        "my-map propagates parameter signals")

# Should have unsandboxed delegation observation
(def obs (get p3 :observations))
(def has-delegation
  (not (empty? (filter (fn [o] (= (get o :kind) :unsandboxed-delegation)) obs))))
(assert has-delegation "my-map has unsandboxed delegation observation")

# ── Closure with mutable capture ────────────────────────────────────────

(def src3
  "
(defn make-counter [start]
  (var n start)
  (defn next [] (assign n (+ n 1)) n)
  next)
")
(def a3 (compile/analyze src3))
(def p4 (portrait:function a3 :next))
(assert (not (empty? (get p4 :captures))) "next has captures")
(assert (not (get (get p4 :composition) :parallelizable))
        "next is not parallelizable (mutable capture)")
(assert (not (get (get p4 :composition) :stateless)) "next is not stateless")

# ── Phase classification ────────────────────────────────────────────────

(def phases (get p1 :phases))
(assert (array? phases) "phases is array")
# add only calls +, which is pure
(when (not (empty? phases))
  (assert (= (get (first phases) :kind) :pure) "add's phase is pure"))

# ── Composition: not retry-safe when I/O ────────────────────────────────

# We can't easily synthesize an I/O function in analyze-only mode
# (println yields), but we can verify the composition logic works.
(def comp
  (portrait:composition {:bits |:io :error|
                         :propagates ||
                         :silent false
                         :yields true
                         :io true
                         :jit-eligible false} []))
(assert (not (get comp :retry-safe)) "I/O function is not retry-safe")
(assert (get comp :timeout-safe) "stateless I/O is timeout-safe")
(assert (get comp :stateless) "no captures means stateless")
(assert (not (get comp :memoizable)) "I/O function is not memoizable")

# ── False-mutable advisory (mutable binding never reassigned) ────────────

# `n` is declared mutable (var) but only read — the linter flags it and the
# module portrait reflects that advisory.
(def src4 "
(defn counter []
  (var n 0)
  n)
")
(def a4 (compile/analyze src4))
(def mp4 (portrait:module a4))
(def fm (get mp4 :false-mutable))
(assert (array? fm) "module portrait has a false-mutable list")
(assert (not (empty? fm)) "var n (never assigned) is flagged false-mutable")
(assert (contains? (get (first fm) :message) "'n'")
        "advisory names the binding n")

# Reflects compile/diagnostics — portrait and the linter agree.
(def has-fm-diag
  (not (empty? (filter (fn [d]
                         (= (get d :rule) "mutable-binding-never-assigned"))
                       (compile/diagnostics a4)))))
(assert has-fm-diag "linter emits the false-mutable diagnostic")

# The render surfaces the advisory text.
(def mod-text4 (portrait:render-module mp4))
(assert (contains? mod-text4 "never reassigned")
        "module render shows the false-mutable section")

# Per-function: counter's own portrait observes the false-mutable n.
(def p-counter (portrait:function a4 :counter))
(def has-fm-obs
  (not (empty? (filter (fn [o] (= (get o :kind) :false-mutable))
                       (get p-counter :observations)))))
(assert has-fm-obs "counter's portrait observes its false-mutable binding")

# Attribution is exact: a function WITHOUT a false-mutable does not inherit
# another function's advisory.
(def a6
  (compile/analyze "
(defn pure-fn [a b] (+ a b))
(defn leaky [] (var m 0) m)
"))
(def clean-obs (get (portrait:function a6 :pure-fn) :observations))
(assert (empty? (filter (fn [o] (= (get o :kind) :false-mutable)) clean-obs))
        "pure-fn does not inherit leaky's false-mutable advisory")

# Negative: an immutable binding holding a MUTABLE VALUE is not flagged.
# `buf` never changes as a binding; only its (mutable string) value would.
(def src5 "
(defn build []
  (let [buf @\"\"]
    buf))
")
(def a5 (compile/analyze src5))
(assert (empty? (get (portrait:module a5) :false-mutable))
        "immutable binding of a mutable value is not a false-mutable")

# ── Unused-binding advisory ─────────────────────────────────────────────

# `spare` is bound and never read. The linter flags it and the portrait
# reflects the flag, at both granularities.
(def a7 (compile/analyze "
(defn tidy [] (let [spare 1] 2))
(tidy)
"))
(def ub (get (portrait:module a7) :unused-binding))
(assert (array? ub) "module portrait has an unused-binding list")
(assert (not (empty? ub)) "spare is flagged unused")
(assert (contains? (get (first ub) :message) "'spare'")
        "advisory names the binding spare")

(def tidy-obs (get (portrait:function a7 :tidy) :observations))
(assert (not (empty? (filter (fn [o] (= (get o :kind) :unused-binding)) tidy-obs)))
        "tidy's portrait observes its own unused binding")

(assert (contains? (portrait:render-module (portrait:module a7)) "never used")
        "module render shows the unused-binding section")

# ── Non-tail-recursion advisory ─────────────────────────────────────────

(def a8
  (compile/analyze "
(defn depth [n] (if (= n 0) 0 (+ 1 (depth (- n 1)))))
"))
(def ntr (get (portrait:module a8) :non-tail-recursion))
(assert (not (empty? ntr)) "depth's self-call is flagged")

(def depth-obs (get (portrait:function a8 :depth) :observations))
(assert (not (empty? (filter (fn [o] (= (get o :kind) :non-tail-recursion))
                             depth-obs)))
        "depth's portrait observes its own non-tail recursion")

# Attribution is exact — the accumulator form inherits nothing.
(def a9
  (compile/analyze "
(defn depth [n] (if (= n 0) 0 (+ 1 (depth (- n 1)))))
(defn total [n acc] (if (= n 0) acc (total (- n 1) (+ acc 1))))
"))
(def total-obs (get (portrait:function a9 :total) :observations))
(assert (empty? (filter (fn [o] (= (get o :kind) :non-tail-recursion)) total-obs))
        "total does not inherit depth's advisory")

(println "all portrait tests passed")
