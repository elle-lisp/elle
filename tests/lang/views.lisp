(elle/epoch 19)
# audited: 2026-11-24
# match reads a syntax object as syntax/e answers it, and each name binds the node its place holds.
# docs/macros/syntax.md
#
# Most subjects are built at run time with syntax/<-datum, so every tier runs
# them. The macros at the end run the same rule inside a transformer, where the
# nodes carry spans and the caller's scopes.

(defn node [datum]
  (syntax/<-datum nil datum))

# tests/lang/syntax-predicates.lisp covers what syntax/e answers for each kind.

# ── match reads a node and binds what each place holds ──────────────

(assert (= (match (node 'x)
             x (type-of x)) :syntax)
        "a variable at the top binds the syntax object itself")

(assert (= (match (node (list 'f 1 2))
             (f a b) [(syntax/e f) (syntax/e a) (syntax/e b)]
             _ :no) ['f 1 2])
        "a list pattern matches a list node, one name per child")

(assert (= (match (node (list (list 'a 'b) 'c))
             (h c) [(type-of h) (syntax/kind h)]
             _ :no) [:syntax :list])
        "a name at a compound place binds the node, not its view")

(assert (= (match (node (list 'f 1 2 3))
             (_ & args)
               [(type-of args) (length args) (type-of (first args))]
             _ :no) [:list 3 :syntax])
        "a list rest binds a list of the remaining children")

(assert (= (match (node (list 'f 1))
             (a b c) :three
             (a b) :two
             _ :no) :two) "a list pattern of another length does not match")

(assert (= (match (node [1 2])
             (a b) :list
             [a b] :array
             _ :no) :array)
        "an array node matches an array pattern and not a list pattern")

(assert (= (match (node (list 1 2))
             [a b] :array
             (a b) :list
             _ :no) :list)
        "a list node matches a list pattern and not an array pattern")

(assert (= (match (node [1 2 3])
             [a & more] [(type-of more) (length more)]
             _ :no) [:array 2])
        "an array rest binds an array of the remaining children")

(assert (= (match (node {:r 5 :x 0})
             {:r r} (syntax/e r)
             _ :no) 5) "a struct pattern matches a struct node by datum key")

(assert (= (match (node {:x 0})
             {:r r} :has-r
             _ :no) :no) "a struct pattern needs its key in the node")

# ── Literals compare with an atom node's value ───────────────────────

(assert (= (match (node 0)
             0 :zero
             _ :other) :zero) "an int literal matches an int node")
(assert (= (match (node 1)
             0 :zero
             _ :other) :other) "an int literal does not match another int node")
(assert (= (match (node (list 'f))
             0 :zero
             _ :other) :other) "a literal does not match a compound node")
(assert (= (match (node nil)
             nil :nil
             _ :other) :nil) "the nil pattern matches a nil node")
(assert (= (match (node false)
             true :t
             false :f) :f) "a boolean literal matches a boolean node")

(assert (= (match (node (list 1 1.5 "s" :k true nil))
             (1 1.5 "s" :k true nil) :all
             _ :no) :all) "each literal kind matches its node inside a list")

# ── Quoted symbols, or-patterns and guards ──────────────────────────

(defn head [stx]
  (match stx
    ('if c t e) :if
    ('if c t) :one-armed
    (h & _) :call
    _ :atom))

(assert (= (head (node (list 'if 'a 'b 'c))) :if)
        "a quoted symbol matches a symbol node of its name")
(assert (= (head (node (list 'if 'a 'b))) :one-armed)
        "a quoted-symbol arm of another length gives way")
(assert (= (head (node (list 'when 'a 'b))) :call)
        "a quoted symbol does not match a symbol node of another name")
(assert (= (head (node (list "if" 'a 'b 'c))) :call)
        "a quoted symbol does not match a string node")
(assert (= (head (node 'if)) :atom) "a symbol node is no list")

(assert (= (match (node (list 'when 'c 'x))
             (or ('if c t) ('when c t)) (syntax/e c)
             _ :no) 'c) "an or-pattern tries each alternative against the node")

(assert (= (match (node (list 'g 1))
             (h & _) when
             (= (syntax/e h) 'f) :f
             (h & _) when
             (= (syntax/e h) 'g) :g
             _ :no) :g) "a guard reads the nodes its pattern bound")

# A guard that may suspend compiles the match arm by arm instead of as a
# decision tree. The counter-factual is a view taken on one path and not the
# other: the fiber's answer then differs from the plain call's.
(defn head-when [stx ok?]
  (match stx
    ('f x) when
    (ok? x) (syntax/e x)
    ('f & _) :other-f
    _ :no))

(assert (= (head-when (node (list 'f 7)) (fn [_] true)) 7)
        "a node matches beside a guard that may suspend")
(assert (= (head-when (node (list 'f 7)) (fn [_] false)) :other-f)
        "a failed guard that may suspend gives way to the next arm")
(assert (= (head-when (node (list 'g 7)) (fn [_] true)) :no)
        "a quoted symbol fails beside a guard that may suspend")

(def guarded
  (fiber/new (fn []
               (head-when (node (list 'f 7))
                          (fn [x]
                            (emit :yield (type-of x))
                            true))) |:yield|))
(assert (= (fiber/resume guarded) :syntax)
        "a suspending guard sees the bound node")
(assert (= (fiber/resume guarded) 7)
        "the arm answers once its suspended guard resumes")

# ── A plain value that holds nodes ──────────────────────────────────

(assert (= (match (list (node 1) (node (list 'a 'b)))
             (1 (x y)) [(syntax/e x) (syntax/e y)]
             _ :no) ['a 'b]) "the nodes inside a plain list read the same way")

(assert (= (match (list 1 (list 'a 'b))
             (1 ('a y)) y
             _ :no) 'b) "a plain value with no node matches as data")

# ── No arm matches ──────────────────────────────────────────────────

(let [[ok? err] (protect (match (node (list 'f))
                           (a b) :two))]
  (assert (not ok?) "a node no arm matches signals")
  (assert (= (get err :error) :match-error) "the signal is a :match-error")
  (assert (= (type-of (get err :value)) :syntax)
          "the error carries the scrutinee as it was given"))

# ── Inside a transformer ────────────────────────────────────────────

(defmacro swap-args (call)
  (match call
    (f a b)
      `(,f ,b ,a)
    _ (syntax/error call "wants a call of two arguments")))
(assert (= (swap-args (- 1 10)) 9) "a transformer destructures its argument")

(defmacro second-arg (call)
  (match call
    (_ _ b) b))
(let [y 7]
  (assert (= (second-arg (f x y)) 7)
          "a bound node keeps the caller's scopes, so it resolves to the caller's binding"))

(defmacro arg-line (call)
  (match call
    (_ a) (get (syntax/span a) :line)))
(assert (= (arg-line (f 1)) (get (meta/location) :line))
        "a bound node keeps its span")

(defmacro first-name (& forms)
  (match forms
    ([name _ & _] & _) (list 'quote name)))
(assert (= (first-name [x 1] [y 2]) 'x)
        "the elements of a rest parameter read as nodes")

(defmacro an-if? (form)
  (match form
    ('if & _) true
    _ false))
(assert (an-if? (if a b c)) "a quoted symbol matches a macro argument's head")
(assert (not (an-if? (cond
                       a b))) "and refuses another head")

(defmacro is-zero (x)
  (match x
    0 :zero
    _ :other))
(assert (= [(is-zero 0) (is-zero 1) (is-zero (f))] [:zero :other :other])
        "a literal matches an atom argument")
