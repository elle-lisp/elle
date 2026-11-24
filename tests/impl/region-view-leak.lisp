(elle/epoch 19)
# audited: 2026-11-24
# The views a match takes of a syntax object free with the node, so a loop that matches a fresh node each pass stays bounded.
# docs/macros/syntax.md
#
# A view lives in the region of the node it reads and needs no release of its
# own. The counterfactual is a view built in a region that no release reaches:
# every pass then strands one view per step the match takes, and the delta
# over the window reads at least the window's length.
#
# region-view-uaf.lisp is the soundness complement.

(include-file "gauge/window.lisp")

(def window 2000)
(defn node [datum]
  (syntax/<-datum nil datum))

# subjects ─────────────────────────────────────────────────────────────

# (a) a list pattern that reads a child.
(defn s-list []
  (match (node (list 'f 1 2))
    (_ a b) (syntax/e b)
    _ :fail))

# (b) a rest the view builds.
(defn s-rest []
  (match (node (list 'f 1 2 3))
    (_ & args) (length args)
    _ :fail))

# (c) a struct node.
(defn s-struct []
  (match (node {:k 1})
    {:k v} (syntax/e v)
    _ :fail))

# (d) arms that fail before one matches, each taking views on its way.
(defn s-fallthrough []
  (match (node (list 'g (list 1 2)))
    ('f x) :f
    [a b] :array
    (_ (p q r)) :three
    (_ (p q)) (syntax/e q)
    _ :fail))

# (e) a guard that may suspend, which compiles the match arm by arm.
(defn s-sequential [ok?]
  (match (node (list 'f 1))
    ('f x) when
    (ok? x) (syntax/e x)
    _ :fail))
(defn s-sequential-pass []
  (s-sequential (fn [_] true)))

# controls ─────────────────────────────────────────────────────────────

(defn c-build []
  (node (list 'f 1 2))
  :built)
(defn c-children []
  (length (syntax/children (node (list 'f 1 2)))))

(def bounded? (bounded-by 100))

(bounded? (measure c-build 200 window) "control: a node built and dropped")
(bounded? (measure c-children 200 window) "control: a node's children read")

(bounded? (measure s-list 200 window) "a list pattern's views")
(bounded? (measure s-rest 200 window) "a list rest's view")
(bounded? (measure s-struct 200 window) "a struct pattern's view")
(bounded? (measure s-fallthrough 200 window) "views taken by arms that fail")
(bounded? (measure s-sequential-pass 200 window)
          "views taken by a match compiled arm by arm")

# Value preservation: the views must not change what the match answers.
(assert (= (s-list) 2) "a list pattern read the wrong child")
(assert (= (s-rest) 3) "a list rest held the wrong children")
(assert (= (s-struct) 1) "a struct pattern read the wrong value")
(assert (= (s-fallthrough) 2) "the arms fell through to the wrong one")
(assert (= (s-sequential-pass) 1) "the arm-by-arm match read the wrong child")

(println "region-view-leak: ok")
