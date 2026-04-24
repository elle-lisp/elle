(elle/epoch 9)
# Tests for using fibers and closures as @struct keys


# ============================================================================
# Fiber keys
# ============================================================================

(assert (= (let [f (fiber/new (fn () 1) 0)]
    (let [t @{}]
      (put t f :running)
      (get t f))) :running) "fiber as @struct key")

(assert (= (let [f (fiber/new (fn () 1) 0)]
    (let [t @{}]
      (put t f 1)
      (put t f 2)
      (get t f))) 2) "fiber key overwrites same key")

(assert (= (let [f1 (fiber/new (fn () 1) 0)
        f2 (fiber/new (fn () 2) 0)]
    (let [t @{}]
      (put t f1 :a)
      (put t f2 :b)
      (get t f1))) :a) "different fibers are different keys")

(assert (= (let [f (fiber/new (fn () 1) 0)]
    (let [t @{}]
      (put t f 1)
      (has? t f))) true) "has-key with fiber")

(assert (= (let [f (fiber/new (fn () 1) 0)]
    (let [t @{}]
      (put t f 1)
      (del t f)
      (has? t f))) false) "del with fiber key")

(assert (= (let [f (fiber/new (fn () 1) 0)]
    (let [t @{}]
      (put t f 1)
      (identical? (first (keys t)) f))) true) "keys roundtrip identity fiber")

(assert (= (let [f (fiber/new (fn () 1) 0)]
    (let [s (struct f :val)]
      (get s f))) :val) "fiber as struct key")

# ============================================================================
# Closure keys
# ============================================================================

(assert (= (let [c (fn () 1)]
    (let [t @{}]
      (put t c :meta)
      (get t c))) :meta) "closure as @struct key")

(assert (= (let [c (fn () 1)]
    (let [t @{}]
      (put t c 1)
      (put t c 2)
      (get t c))) 2) "closure key overwrites same key")

(assert (= (let [c1 (fn () 1)
        c2 (fn () 2)]
    (let [t @{}]
      (put t c1 :a)
      (put t c2 :b)
      (get t c1))) :a) "different closures are different keys")

(assert (= (let [c (fn () 1)]
    (let [t @{}]
      (put t c 1)
      (identical? (first (keys t)) c))) true) "keys roundtrip identity closure")

(assert (= (let [c (fn () 1)]
    (let [s (struct c :val)]
      (get s c))) :val) "closure as struct key")

# ============================================================================
# Mixed keys
# ============================================================================

(assert (= (let [f (fiber/new (fn () 1) 0)
        c (fn () 2)]
    (let [t @{}]
      (put t :name "proc")
      (put t f :fiber-data)
      (put t c :closure-data)
      (get t f))) :fiber-data) "mixed keys fiber closure keyword")

(assert (= (let [f (fiber/new (fn () 1) 0)
        c (fn () 2)]
    (let [t @{}]
      (put t f :fib)
      (put t c :clo)
      (list (get t f) (get t c)))) (list :fib :clo)) "fiber and closure are different keys")

# ============================================================================
# Error tests (from integration/table_keys.rs)
# ============================================================================

# rejected_type_still_errors
(let [[ok? _] (protect ((fn ()
  (let [t @{}]
    (put t @[1 2] :val)))))] (assert (not ok?) "unhashable @array as @struct key errors"))
