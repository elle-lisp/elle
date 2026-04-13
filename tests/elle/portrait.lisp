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
# add has SIG_ERROR (from +) so not strictly silent, but jit-eligible
(assert (not (get (get p1 :signal) :silent)) "add has SIG_ERROR from +")
(assert (empty? (get p1 :captures)) "add has no captures")
# add has SIG_ERROR so portrait considers it non-memoizable (conservative)
(assert (not (get (get p1 :composition) :memoizable)) "add not memoizable (has SIG_ERROR)")
(assert (get (get p1 :composition) :parallelizable) "add is parallelizable")
(assert (get (get p1 :composition) :jit-eligible) "add is jit-eligible")
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

(def src2 "
(defn my-map [f lst]
  (if (empty? lst)
    ()
    (cons (f (first lst)) (my-map f (rest lst)))))
")
(def a2 (compile/analyze src2))
(def p3 (portrait:function a2 :my-map))

# my-map propagates parameter 0's signals
(assert (not (empty? (get (get p3 :signal) :propagates)))
  "my-map propagates parameter signals")

# Should have unsandboxed delegation observation
(def obs (get p3 :observations))
(def has-delegation (not (empty?
  (filter (fn [o] (= (get o :kind) :unsandboxed-delegation)) obs))))
(assert has-delegation "my-map has unsandboxed delegation observation")

# ── Closure with mutable capture ────────────────────────────────────────

(def src3 "
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
(assert (not (get (get p4 :composition) :stateless))
  "next is not stateless")

# ── Phase classification ────────────────────────────────────────────────

(def phases (get p1 :phases))
(assert (array? phases) "phases is array")
# add only calls +, which is pure
(when (not (empty? phases))
  (assert (= (get (first phases) :kind) :pure) "add's phase is pure"))

# ── Composition: not retry-safe when I/O ────────────────────────────────

# We can't easily synthesize an I/O function in analyze-only mode
# (println yields), but we can verify the composition logic works.
(def comp (portrait:composition
            {:bits |:io :error| :propagates || :silent false
             :yields true :io true :jit-eligible false}
            []))
(assert (not (get comp :retry-safe)) "I/O function is not retry-safe")
(assert (get comp :timeout-safe) "stateless I/O is timeout-safe")
(assert (get comp :stateless) "no captures means stateless")
(assert (not (get comp :memoizable)) "I/O function is not memoizable")

(println "all portrait tests passed")
