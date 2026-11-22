(elle/epoch 18)
# audited: 2026-11-21
# compile/primitives answers each Rust primitive's metadata, its declared signature included.
# docs/spec/form.md

(def prims (compile/primitives))

# ── Basic structure ────────────────────────────────────────────────────

(assert (array? prims) "compile/primitives returns an array")
(assert (> (length prims) 300) "at least 300 primitives exist")

# ── Each entry has required keys ──────────────────────────────────────

(def first-prim (get prims 0))
(assert (string? (get first-prim :name)) "entry has :name string")
(assert (string? (get first-prim :category)) "entry has :category string")
(assert (string? (get first-prim :arity)) "entry has :arity string")
(assert (struct? (get first-prim :signal)) "entry has :signal struct")
(assert (string? (get first-prim :doc)) "entry has :doc string")
(assert (array? (get first-prim :params)) "entry has :params array")
(assert (array? (get first-prim :aliases)) "entry has :aliases array")

# ── Find primitives by name ────────────────────────────────────────────

(defn find-prim [name]
  (def @result nil)
  (each p in prims
    (when (= (get p :name) name) (assign result p)))
  result)

# apply is a macro (prelude.lisp), not a Rust primitive
(assert (nil? (find-prim "apply"))
        "apply is not a Rust primitive (it's a macro)")

# pair and + are stdlib functions (not Rust primitives)
(assert (nil? (find-prim "pair")) "pair is stdlib, not a Rust primitive")
(assert (nil? (find-prim "+")) "+ is stdlib, not a Rust primitive")

# math/inf is a core Rust primitive
(def inf-prim (find-prim "math/inf"))
(assert (not (nil? inf-prim)) "math/inf primitive exists")
(assert (= (get inf-prim :category) "math") "math/inf is in math category")

# ── Find port/write (yields, io) ──────────────────────────────────────

(def pw-prim (find-prim "port/write"))
(assert (not (nil? pw-prim)) "port/write primitive exists")
(def pw-sig (get pw-prim :signal))
(assert (not (get pw-sig :silent)) "port/write is not silent")
(assert (get pw-sig :yields) "port/write yields")
(assert (get pw-sig :io) "port/write has io")

# ── compile/primitives is itself in the list ──────────────────────────

(def self-prim (find-prim "compile/primitives"))
(assert (not (nil? self-prim)) "compile/primitives is in its own output")
(assert (get (get self-prim :signal) :silent) "compile/primitives is silent")
(assert (= (get self-prim :category) "compile")
        "compile/primitives is in compile category")

# ── Declared signatures ───────────────────────────────────────────────

(defn declares? [name params rest result]
  "Whether the native NAME declares PARAMS, REST and RESULT as its :spec."
  (def spec (get (find-prim name) :spec))
  (and (struct? spec) (= (get spec :params) params) (= (get spec :rest) rest)
       (= (get spec :result) result)))

(assert (declares? "fiber/resume" [:resumed] :crosses [:payload 0])
        "fiber/resume resumes its fiber, passes its value on, and answers the payload")
(assert (declares? "fiber/value" [:borrowed] nil [:payload 0])
        "fiber/value answers the payload of its fiber")
(assert (declares? "fiber/status" [:borrowed] nil :immediate)
        "fiber/status answers an immediate")
(assert (declares? "list" [] :returned :fresh)
        "a fresh list borrows each element")
(assert (declares? "array" [] :returned :fresh)
        "a fresh array borrows each element")
(assert (declares? "@array" [] [:stored :result] :fresh)
        "a fresh @array takes each element")
(assert (declares? "%pair" [:returned :returned] nil :fresh)
        "a fresh pair borrows both halves")
(assert (declares? "%first" [:returned] nil :alias)
        "%first answers a borrow from its pair")
(assert (declares? "insert" [:returned :borrowed [:stored [:param 0]]] nil
                   [:itself 0])
        "insert stores its value into its array, and answers the array itself")

# Each of these answers a different kind on each type or trait it dispatches
# on, so no one declaration holds on every path.
(each name in ["first" "rest" "length" "get"]
  (assert (nil? (get (find-prim name) :spec))
          "a native that dispatches declares nothing"))

(println "compile-primitives: all tests passed")
