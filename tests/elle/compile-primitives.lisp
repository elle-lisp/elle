(elle/epoch 9)
## compile/primitives — verify Rust primitive metadata is accessible

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

# abs is a core Rust primitive (kept in arithmetic.rs)
(def abs-prim (find-prim "abs"))
(assert (not (nil? abs-prim)) "abs primitive exists")
(assert (= (get abs-prim :category) "arithmetic")
        "abs is in arithmetic category")

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

(println "compile-primitives: all tests passed")
