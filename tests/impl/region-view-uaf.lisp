(elle/epoch 19)
# audited: 2026-11-24
# A name match binds through the view of a syntax object keeps the node's region alive through its uses.
# docs/macros/syntax.md
#
# A view lives in the region of the node it reads, so a name bound through one
# borrows the scrutinee, as a name bound to a list element does. Each witness
# binds through a view and reads the name after the match ends. The reader is
# syntax/kind or length, which answers an immediate, so the only region a
# release could free under the read is the scrutinee's.
#
# Counterfactual: a view whose bound names carry no region of the scrutinee
# frees the node at the match's own release point. Under --trace=guardfree the
# read after the match then faults. A fresh node per iteration makes an early
# free fault on the next read.

(defn node [datum]
  (syntax/<-datum nil datum))

# ── witnesses: names bound through a view, read after the match ─────

# (a) a list node's child.
(defn w-list-child []
  (syntax/kind (match (node (list 'f (list 1 2)))
                 (_ x) x
                 _ :fail)))

# (b) a list rest: the list the view built, holding child nodes.
(defn w-list-rest []
  (length (match (node (list 'f 1 2))
            (_ & args) args
            _ :fail)))

# (c) a struct node's value.
(defn w-struct-value []
  (syntax/kind (match (node {:k (list 1)})
                 {:k v} v
                 _ :fail)))

# (d) an array rest.
(defn w-array-rest []
  (length (match (node [1 2 3])
            [a & more] more
            _ :fail)))

# (e) a node two views deep.
(defn w-nested []
  (syntax/kind (match (node (list (list 'a (list 1))))
                 ((_ y)) y
                 _ :fail)))

# (f) a guarded arm.
(defn w-guard []
  (syntax/kind (match (node (list 'f (list 1)))
                 ('f x) when
                 (syntax/list? x) x
                 _ :fail)))

# ── controls: the same reads through syntax/children ────────────────
# A broken harness fails these too.

(defn c-child []
  (syntax/kind (get (syntax/children (node (list 'f (list 1 2)))) 1)))
(defn c-value []
  (syntax/kind (get (syntax/children (node {:k (list 1)})) 1)))

# ── drive: a fresh node each iteration ──────────────────────────────

(var i 0)
(var a nil)
(var b nil)
(var c nil)
(var d nil)
(var e nil)
(var f nil)
(var g nil)
(var h nil)
(while (%lt i 3000)
  (assign a (w-list-child))
  (assign b (w-list-rest))
  (assign c (w-struct-value))
  (assign d (w-array-rest))
  (assign e (w-nested))
  (assign f (w-guard))
  (assign g (c-child))
  (assign h (c-value))
  (assign i (%add i 1)))

(assert (= g :list)
        "control: a child read through syntax/children (harness broken)")
(assert (= h :list)
        "control: a value read through syntax/children (harness broken)")

(assert (= a :list) "a list child bound through a view lost its node")
(assert (= b 2) "a list rest bound through a view lost its children")
(assert (= c :list) "a struct value bound through a view lost its node")
(assert (= d 2) "an array rest bound through a view lost its children")
(assert (= e :list) "a node two views deep lost its node")
(assert (= f :list) "a guarded arm's node bound through a view lost its node")

(println "region-view-uaf: ok")
