(elle/epoch 12)
# A `match`-destructured `rest` alias is a BORROWED subview of the scrutinee
# (the `Rest` intrinsic loads the cdr pointer into the scrutinee's region pages,
# but the region solver only registers a counted container read for *call-site*
# `rest()`/`first()`, not for pattern loads). Passing such an alias as an
# owned-param CALL ARGUMENT makes the callee's param release free
# the caller's still-live scrutinee region — a use-after-free on the caller's
# original list.
#
# Witness: a self-recursive `match` walk passes `rest` to its own tail call;
# the caller's scratch list (built by `map`) must survive the walk intact.
# GREEN since the lowerer marks destructure-rest bindings borrowed
# (`destructure_alias_bindings` → the borrowed-arg incref at the call site).
# RED before the fix (the freed tail prints as <heap:...> / length fails).

(defn find-entry [entries name]
  (match entries
    () nil
    (entry & rest)
      (if (= (get entry :name) name) entry (find-entry rest name))
    _ nil))

# ── witnesses: passing a match-bound alias to an owned-param callee ──────────
# Each witness tail-passes a pattern-bound ELEMENT alias into a self-recursive
# owned-param callee. Without the borrowed marking, the callee's param release
# frees the caller's still-live scrutinee region (stale-region deref / SIGSEGV
# under --trace=guardfree). All four element positions are covered: cons Rest,
# cons First, array Index, struct Key.

# (a) Rest: the tail of a cons (match `(entry & rest)`), self-recursive walk.
(defn consume [entry depth]
  (if (%lt depth 1) (get entry :name) (consume entry (%add depth -1))))

(defn w_rest [entries]
  (match entries
    () nil
    (entry & rest) (consume2 rest 3)
    _ nil))

# (b) First: the HEAD of a cons — the review's minimal head-element witness.
(defn w_first [entries]
  (match entries
    () nil
    (entry & rest) (consume entry 3)
    _ nil))

# (c) Index: an immutable-array element `[a b]`.
(defn consume2 [a depth]
  (if (%lt depth 1) a (consume2 a (%add depth -1))))

(defn w_index [arr]
  (match arr
    [a b] (consume2 a 3)
    _ nil))

# (d) Key: an immutable-struct value `{:k v}`.
(defn consume3 [v depth]
  (if (%lt depth 1) v (consume3 v (%add depth -1))))

(defn w_key [st]
  (match st
    {:k v} (consume3 v 3)
    _ nil))

# (e) Ordering: a `match` compound in TAIL-ARGUMENT position whose arm value is
# the pattern's own rest binding. The call site classifies the argument BEFORE
# the match is lowered, so the alias must be precomputed, not discovered during
# the match walk.
(defn w_order [xs depth]
  (if (%lt depth 1) (length xs) (w_order xs (%add depth -1))))

(defn w_order_driver [entries]
  (w_order (match entries
             () ()
             (a & r) r
             _ ()) 3))
# (f) Destructure: `(def (a & r) xs)` — the binding-destructure route. The
# original patch marked only the cons REST here; the HEAD (`a`) is a First
# element load and needs the same borrowed marking (precompute covers it).
(defn w_destructure_rest [xs]
  (def (a & r) xs)
  (consume2 r 3))

(defn w_destructure_head [xs]
  (def (a & r) xs)
  (consume2 a 3))

(var i 0)
(while (%lt i 2000)
  (let* [r2 (map (fn [u] u) (list {:name "a"} {:name "b"} {:name "c"} {:name "d"}))
         found (find-entry r2 "c")
         arr2 [1 2]]
    (assert (= (get found :name) "c") "find-entry returns the matching entry")
    (assert (= (length r2) 4) "the map-built list survives a tail-recursive match walk")
    (assert (= (length (w_rest r2)) 3) "cons Rest alias survives the owned-param self-call")
    (assert (= (w_first r2) "a") "cons First (head) alias survives the owned-param self-call")
    (assert (= (w_index arr2) 1) "array Index element alias survives the owned-param self-call")
    (assert (= (w_key {:k 99 :j 88}) 99) "struct Key value alias survives the owned-param self-call")
    (assert (= (w_order_driver r2) 3) "ordering: a match-compound tail arg is classified after its aliases are registered")
    (assert (= (length (w_destructure_rest r2)) 3) "destructure rest alias survives the owned-param self-call")
    (assert (= (w_destructure_head r2) {:name "a"}) "destructure head alias survives the owned-param self-call"))
  (assign i (%add i 1)))

(println "region-match-rest-tail-move-uaf: ok")
