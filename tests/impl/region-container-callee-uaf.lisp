(elle/epoch 18)
# audited: 2026-11-23
# A closure that re-stores the container it was called through keeps running on a reference of its own.
# docs/impl/region/reads.md
#
# The sidecar arms guardfree and keeps the JIT off, so the interpreter runs
# every body. Each closure below sets the container that holds it to nil, then
# reads its own capture. Each `make` builds a fresh closure and a fresh capture,
# so the container holds the closure's only other reference.
#
# The counter-factual: the call reads its callee straight out of the container
# and takes no count. The `assign` then frees the running closure and its
# capture, and the read of `x` faults under guardfree.
#
# (c1) a file-level container, called in tail position.
# (c2) a file-level container, called in non-tail position.
# (c3) a container local to a function, which the closure captures.
#
# The expected total is the sum of the lengths of "cap0" to "cap199": ten of
# four characters, ninety of five and a hundred of six.

(def expected 1090)
(def @f nil)

(defn make [i]
  (let [x (string "cap" i)]
    (fn []
      (assign f nil)
      (length x))))

# ── (c1) tail position ──────────────────────────────────────────────────────
(defn tail-go []
  (f))
(def @tail-total 0)
(each i in (range 200)
  (assign f (make i))
  (assign tail-total (+ tail-total (tail-go))))
(assert (= tail-total expected)
        "a tail call through a container keeps its callee")

# ── (c2) non-tail position ──────────────────────────────────────────────────
(defn body-go []
  (+ 0 (f)))
(def @body-total 0)
(each i in (range 200)
  (assign f (make i))
  (assign body-total (+ body-total (body-go))))
(assert (= body-total expected)
        "a non-tail call through a container keeps its callee")

# ── (c3) a container local to a function ────────────────────────────────────
(defn local-run []
  (def @g nil)
  (defn make-local [i]
    (let [x (string "cap" i)]
      (fn []
        (assign g nil)
        (length x))))
  (def @total 0)
  (each i in (range 200)
    (assign g (make-local i))
    (assign total (+ total (g))))
  total)
(assert (= (local-run) expected)
        "a call through a captured local container keeps its callee")

(println "region-container-callee-uaf: ok")
