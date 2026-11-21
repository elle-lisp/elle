(elle/epoch 18)
# audited: 2026-11-21
# compile/lir answers each function of a unit in the data form, naming the binding each slot holds.
# docs/spec/form.md
#
# The extractor that reads a function's facts reads this form, and the release
# diff names each value by the binding the form gives its slot. A form that
# kept fn/flow's Debug strings, or that dropped the slot names, would leave the
# extractor nothing it could read safely.

(def source
  (string "(defn f [coll]\n" "  (def a @[])\n" "  (push a (get coll 0))\n"
          "  (length a))\n" "(defn g [c] (if c (fn [] c) 2))\n"
          "(defn h [] (def s \"hi\") s)\n"
          "(defn m [] (var x [1 2]) (def set-x (fn [] (assign x [3 4])))\n"
          "  (set-x) (match x [p q] p _ 0))\n"))
(def unit (compile/lir source "<compile-lir>"))

(assert (array? unit) "a unit is an array of function structs")
(assert (nil? (get (get unit 0) :id))
        "the entry function comes first, with no closure id")
(assert (= (map (fn [f] (get f :id)) (rest unit)) (range 0 (- (length unit) 1)))
        "the closures follow in the order of their closure ids")

(defn named [name]
  (find (fn [f] (= (get f :name) name)) unit))
(defn instrs [f]
  (mapcat (fn [b] (get b :instrs)) (get f :blocks)))
(defn of-kind [f kind]
  (filter (fn [i] (= (get i :kind) kind)) (instrs f)))
(defn labels [f]
  (map (fn [b] (get b :label)) (get f :blocks)))

# ── A function's header ───────────────────────────────────────────────

(def f (named 'f))
(assert (not (nil? f)) "the binder's name is a symbol")
(assert (= 1 (get f :params)) "f takes one parameter")
(assert (= [1 1] (get f :arity)) "and exactly one")
(assert (= 'coll (get (get f :slots) 0))
        "the parameter holds the slot of its index")
(assert (not (nil? (find (fn [s] (= s 'a)) (get f :slots)))) "a slot holds a")
# `find` answers the element it finds, so a search for nil reads nil whether
# or not it finds one.
(assert (not (empty? (filter nil? (get f :slots))))
        "a temporary's slot has a nil entry")
(assert (not (nil? (find (fn [l] (= l (get f :entry))) (labels f))))
        "the entry is the label of one of the blocks")

# ── Compiler-made bindings ────────────────────────────────────────────

# The test never writes the temp's name, so only compile/lir can teach the
# instance its spelling.
(defn spelling [s]
  "The name s prints as, or nil where the instance cannot spell it."
  (let [[ok? text] (protect (string s))]
    (if ok? text nil)))
(def m (named 'm))
(assert (not (nil? (find (fn [s] (= (spelling s) "__scrutinee")) (get m :slots))))
        "a slot a compiler temp holds reads the temp's name, spelled")
(assert (not (nil? (find (fn [s] (= s :synthetic)) (get m :slots))))
        "a slot a nameless compiler binding holds reads :synthetic")
(each fun in unit
  (each s in (get fun :slots)
    (assert (or (nil? s) (= s :synthetic)
                (and (symbol? s) (string? (spelling s))))
            "every slot reads nil, :synthetic, or a symbol the instance spells")))

# ── Blocks and instructions ───────────────────────────────────────────

(each b in (get f :blocks)
  (assert (integer? (get b :label)) "a block's label is an integer")
  (assert (keyword? (get (get b :term) :kind)) "a terminator names its kind"))
(each i in (instrs f)
  (assert (keyword? (get i :kind)) "an instruction names its kind")
  (def at (get i :at))
  (assert (or (nil? at) (and (= 2 (length at)) (integer? (get at 0))))
          "an instruction's position is [line col] or nil"))
(assert (not (empty? (of-kind f :store-local))) "f stores into its slots")
(assert (not (nil? (find (fn [b] (= :return (get (get b :term) :kind)))
                         (get f :blocks)))) "f returns")

# ── Branches and closures ─────────────────────────────────────────────

(def g (named 'g))
(def branch
  (get (find (fn [b] (= :branch (get (get b :term) :kind))) (get g :blocks))
       :term))
(assert (integer? (get branch :cond)) "a branch names its condition's register")
(each k in [:then-label :else-label]
  (assert (not (nil? (find (fn [l] (= l (get branch k))) (labels g))))
          "a branch names labels of its own function's blocks"))
(def make (get (of-kind g :make-closure) 0))
(def inner (find (fn [fun] (= (get fun :id) (get make :closure-id))) unit))
(assert (= ['c] (get inner :captures))
        "the closure a make-closure names captures c")

# ── Values ─────────────────────────────────────────────────────────────

(def h (named 'h))
(assert (= "hi" (get (get (of-kind h :materialize-const) 0) :template))
        "a heap literal's template is the literal")
(def prims (compile/primitives))
(defn prim [name]
  (find (fn [p] (= (get p :name) name)) prims))
(def length-fn (get (prim "length") :fn))
(assert (not (nil? (find (fn [i] (= (get i :value) length-fn))
                         (of-kind f :value-const))))
        "a value-const holds the native function compile/primitives names")

# ── Region declarations ───────────────────────────────────────────────

(def pop-prim (prim "%pop"))
(assert (= :pass-through (get pop-prim :effect))
        "%pop passes an argument's element through")
(assert (get pop-prim :moves-out) "and takes it out")
(assert (= [] (get pop-prim :effect-args)) "naming no argument")
(def traits-prim (prim "with-traits"))
(assert (= :fresh (get traits-prim :effect)) "with-traits makes a fresh result")
(assert (= [1] (get traits-prim :embeds)) "which embeds its table")
(assert (not (get traits-prim :moves-out)) "and takes nothing out")
(assert (= [:sends [1]]
           [(get (prim "chan/send") :effect)
            (get (prim "chan/send") :effect-args)])
        "chan/send sends its message")
(assert (= [:delivers [1]]
           [(get (prim "fiber/resume") :effect)
            (get (prim "fiber/resume") :effect-args)])
        "fiber/resume delivers its value")

# ── Refusals ───────────────────────────────────────────────────────────

(def [ok? err] (protect (compile/lir 1 "<compile-lir>")))
(assert (and (not ok?) (= :type-error (get err :error)))
        "a source that is no string is refused")
(def [ok2? err2] (protect (compile/lir "(defn" "<compile-lir>")))
(assert (and (not ok2?) (= :compile-error (get err2 :error)))
        "a source that does not compile is refused")

(println "compile-lir: ok")
