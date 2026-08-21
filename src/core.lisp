(elle/epoch 12)
## Pre-prelude definitions
##
## Compiled and executed before the prelude loads.
## Only raw special forms and %-prefixed primitives are available.
## Provides functions that prelude macros need at expansion time.

(def last
  (fn [coll]
    "Return the last element of a sequence. Signals :argument-error if the
     sequence is empty."
    (if (%eq (length coll) 0)
      (emit :error {:error :argument-error :message "last: empty sequence"})
      (get coll (%sub (length coll) 1)))))

(def butlast
  (fn [coll]
    "Return a new sequence with the last element removed. An empty sequence
     yields an empty slice."
    (let [n (length coll)]
      (if (%eq n 0) (slice coll 0 0) (slice coll 0 (%sub n 1))))))

## ── Helpers (not exported) ─────────────────────────────────────────
## core.lisp uses %array-push/%put/%string-push/%bytes-push directly
## (not the user-facing push/put) because push/put are defined in
## stdlib.lisp for the region solver to inline.

(def core-push
  (fn [coll val]
    "Push one element onto an array/string/bytes collection (mutable or
     immutable), dispatching to the %-primitive for its type. Internal
     pre-prelude helper; the user-facing push lives in stdlib."
    (match (type-of coll)
      :array (%array-push coll val)
      :@array (%array-push coll val)
      :string (%string-push coll val)
      :@string (%string-push coll val)
      :bytes (%bytes-push coll val)
      :@bytes (%bytes-push coll val)
      _
        (emit :error {:error :type-error
                      :message (string "push: unsupported type " (type coll))}))))

## BULK for byte-family sources (string/@string/bytes/@bytes); index walk only
## for arrays. `%string-push` and `%bytes-push` each append a whole same-family
## value's raw bytes in ONE shot (string concat == UTF-8 byte concat; bytes
## concat == byte concat), so a string OR bytes source bulk-appends in O(n) with
## a single memcpy. Walking such a source element-by-element instead costs one
## interpreted push PER BYTE — orders of magnitude slower on binary payloads
## (the HTTP/2 body-copy path: frame read-exact accumulates the body with
## `append`). append/concat only ever call push-all with a same-family (dst,
## src), so a byte-family src always has a byte-family dst core-push can extend.
##
## ARRAYS stay on the index walk: their elements are Values, and core-push →
## %array-push carries the per-element cross-region RC/edge accounting each one
## needs. Index-based, NOT (first/rest) recursion — `rest` on an array copies
## the tail, so a (->array src)+rest walk is O(n²) time and allocates a
## throwaway array per element. `(get src i)` on an array is O(1).
(def push-all
  (fn [dst src]
    "Append every element of `src` onto `dst` in place, returning `dst`. A
     byte-family source (string/@string/bytes/@bytes) is bulk-appended in one
     pass; an array is walked by index. Internal helper for append/concat."
    (let [ts (type-of src)]
      (if (if (%eq ts :string)
            true
            (if (%eq ts :@string)
              true
              (if (%eq ts :bytes) true (%eq ts :@bytes))))
        (begin
          (core-push dst src)
          dst)
        (let [n (length src)]
          (letrec [go (fn [i]
                        (if (%lt i n)
                          (begin
                            (core-push dst (get src i))
                            (go (%add i 1)))
                          dst))]
            (go 0)))))))

(def merge-into
  (fn [dst src]
    "Copy every key/value of struct `src` into the mutable struct `dst` in
     place, returning `dst`. Internal helper for append/concat."
    (letrec [go (fn [ks]
                  (if (empty? ks)
                    dst
                    (begin
                      (%put dst (first ks) (get src (first ks)))
                      (go (rest ks)))))]
      (go (keys src)))))

(def core-add
  (fn [coll val]
    "Add one element to a set (mutable or immutable), dispatching to the
     %-primitive for its type. Internal pre-prelude helper; the user-facing
     `add` lives in stdlib. A mutable @set inserts in place and increfs the
     inserted element's region (Rule 5 mutable-store RC); an immutable set
     returns a fresh copy."
    (match (type-of coll)
      :@set (%add-set-mut coll val)
      :set (%add-set coll val)
      _
        (emit :error {:error :type-error
                      :message (string "add: expected set, got " (type coll))}))))

## In-place set union: add every element of `src` into the mutable set `dst`,
## returning `dst`. The set analog of `merge-into` (struct) and `push-all`
## (array/string/bytes). `core-add` inserts into a mutable @set in place and
## increfs the inserted element's region (Rule 5 mutable-store RC), so a
## displaced/shared element is reference-counted correctly. `append`'s `:@set`
## branch uses this so a mutable first argument is mutated in place rather than
## replaced by a fresh `(union a b)` — matching every other mutable container.
(def union-into
  (fn [dst src]
    "Add every element of set `src` into the mutable set `dst` in place,
     returning `dst`. The set analog of merge-into/push-all; used by append's
     :@set branch. Internal helper."
    (let [arr (->array src)
          n (length arr)]
      (letrec [go (fn [i]
                    (if (%lt i n)
                      (begin
                        (core-add dst (get arr i))
                        (go (%add i 1)))
                      dst))]
        (go 0)))))

## ── reverse ────────────────────────────────────────────────────────

(def reverse
  (fn [coll]
    "Return a reversed copy of a sequence (list, array, string, or bytes).
     Lists/syntax return a new list; other sequences return a new immutable
     value of the same family. Signals :type-error for a non-sequence."
    (let [t (type-of coll)]
      (if (if (%eq t :list) true (%eq t :syntax))
        (letrec [go (fn [xs acc]
                      (if (empty? xs)
                        acc
                        (go (rest xs) (%pair (first xs) acc))))]
          (go coll ()))
        (let [n (length coll)
              r (match t
                  :array (@array)
                  :@array (@array)
                  :string (@string)
                  :@string (@string)
                  :bytes (@bytes)
                  :@bytes (@bytes)
                  _
                    (emit :error {:error :type-error
                                  :message (string "reverse: expected sequence, got "
                                  t)}))]
          (letrec [go (fn [i]
                        (if (%lt i 0)
                          nil
                          (begin
                            (core-push r (get coll i))
                            (go (%sub i 1)))))]
            (go (%sub n 1)))
          (match t
            :array (freeze r)
            :string (freeze r)
            :bytes (freeze r)
            _ r))))))

## ── fold / reduce ──────────────────────────────────────────────────

## Normalize once with `->array`, then walk by INDEX — never (first/rest)
## recursion (`rest` on an array copies the tail into a fresh slice per step: F1a
## transform-scratch, O(n²) time + a throwaway slice per element; `(get arr i)` is
## O(1)). The combiner is THREADED through `core-fold-step`, a self-recursive
## top-level binding — no per-call closure at all. This form was long avoided for
## a UAF that turned out to be the const tail-arg borrow (`arg_leaf_is_borrowed`,
## src/lir/lower/control.rs; pinned by region-const-tail-move-borrow-uaf.lisp) —
## a caller tail-moving a stdlib-constant combiner it never owned — not a
## closure-lifetime property of threading. A rewrite here is re-measured against
## the oracle's fold/reduce pins and is NOT verified on a small run: that fault
## was state-dependent (it fired only once region ids recycled onto the freed
## one — deep churn only).
(def core-fold-step
  (fn [f arr n i acc]
    "Index-walk left-fold driver shared by fold/reduce1. A self-recursive
     top-level binding is cell-free (self-call re-dispatch), so a fold call
     allocates NO per-call closure — the letrec-go form this replaces
     allocated a closure+env per call. Threading `f` is sound: a tail-moved
     arg the frame does not own gets a fresh owning reference (docs/impl/
     region/rules.md Rule 5, the borrowed tail-call argument)."
    (if (%lt i n)
      (core-fold-step f arr n (%add i 1) (f acc (get arr i)))
      acc)))

(def fold
  (fn [f init coll]
    "Left-fold `f` over `coll` from the seed `init`:
     (f (f (f init e0) e1) e2)…. Returns `init` unchanged for an empty
     collection. `f` is called as (f acc element)."
    (let [arr (->array coll)
          n (length arr)]
      (core-fold-step f arr n 0 init))))

(def reduce fold)

(def reduce1
  (fn [f coll]
    "Left-fold `f` over `coll` using its first element as the seed:
     (f (f e0 e1) e2)…. Signals :argument-error on an empty collection.
     Internal helper (the user-facing reduce/reduce1 live in stdlib)."
    ## Index walk from element 1, seeded by element 0 — never (rest arr), which
    ## would mint a throwaway tail slice (the same F1a scratch fold avoids above).
    (let [arr (->array coll)
          n (length arr)]
      (if (%eq n 0)
        (emit :error {:error :argument-error
                      :message "reduce1: empty collection"})
        (core-fold-step f arr n 1 (get arr 0))))))

## ── append ─────────────────────────────────────────────────────────
## :syntax branch handles syntax lists from quasiquote expansion.

(def append-list
  (fn [a b]
    "Append two lists (or syntax-lists) into a new list, preserving order.
     Internal helper for append's list/syntax branches."
    (letrec [collect (fn [xs acc]
                       (if (empty? xs)
                         acc
                         (collect (rest xs) (%pair (first xs) acc))))
             build (fn [xs acc]
                     (if (empty? xs)
                       acc
                       (build (rest xs) (%pair (first xs) acc))))]
      (build (collect b (collect a ())) ()))))

# Type compatibility for append/concat: same type, list↔syntax,
# or types differing only in mutability class.
(def append-types-ok?
  (fn [ta tb]
    "True if values of types `ta` and `tb` may be appended/concatenated:
     the same type, list↔syntax, or two values differing only in mutability
     (e.g. :array and :@array). Internal helper."
    (if (%eq ta tb)
      true
      (match [ta tb]
        [:list :syntax] true
        [:syntax :list] true
        [:array :@array] true
        [:@array :array] true
        [:string :@string] true
        [:@string :string] true
        [:bytes :@bytes] true
        [:@bytes :bytes] true
        [:struct :@struct] true
        [:@struct :struct] true
        [:set :@set] true
        [:@set :set] true
        _ false))))

(def append
  (fn [a b]
    "Append b onto a; a and b must be the same family (mutability may differ).

     A MUTABLE first argument (@array, @string, @bytes, @set, @struct) is
     mutated in place and returned — the result is the same object. An
     immutable first argument (list, array, string, bytes, set, struct) is
     left untouched and a new value of the same type is returned. Lists always
     return a new list. For type-mismatched arguments, signals :type-error."
    (let [ta (type-of a)]
      (if (%not (append-types-ok? ta (type-of b)))
        (emit :error {:error :type-error
                      :message (string "append: type mismatch — " ta " vs "
                                       (type-of b))})
        ## Dispatch on the `ta` alias of `(type-of a)`: a keyword arm narrows `a`
        ## authoritatively (the let-alias resolves to `a` — `collect_typeof_aliases`),
        ## which proves the container arguments of the %-store helpers below.
        (match ta
          :list (append-list a b)
          :syntax (append-list a b)
          :array
            (let [r (@array)]
              (push-all r a)
              (push-all r b)
              (freeze r))
          :@array (begin
                    (push-all a b)
                    a)
          :string (string a b)
          :@string (begin
                     (push-all a b)
                     a)
          :bytes
            (let [r (@bytes)]
              (push-all r a)
              (push-all r b)
              (freeze r))
          :@bytes (begin
                    (push-all a b)
                    a)
          :set (union a b)
          :@set (begin
                  (union-into a b)
                  a)
          :struct
            (let [r (@struct)]
              (merge-into r a)
              (merge-into r b)
              (freeze r))
          :@struct (begin
                     (merge-into a b)
                     a)
          _
            (emit :error {:error :type-error
                          :message (string "append: unsupported type " ta)}))))))

## ── concat ─────────────────────────────────────────────────────────
## Single linear pass into one accumulator. Folding `append` pairwise
## (the old impl) rebuilt and re-froze a growing intermediate per
## argument — O(n²) copies, AND every intermediate landed in the same
## never-freed region, so `(apply concat chunks)` over N byte chunks
## leaked O(n²) memory (50 KB result → ~16 GiB, OOM). For the push-all
## sequence types (array/string/bytes, mutable or immutable) we fill one
## accumulator and freeze once. Immutable first arg → fresh accumulator,
## frozen result (a is copied in, unchanged). Mutable first arg → mutate
## a in place and return it, matching `append`'s @-variant behaviour.
## list/syntax/set/struct keep the pairwise fold (no push-all path).

(def concat-seq
  (fn [a rest acc fresh?]
    "Concatenate push-all sequence `a` and the sequences in `rest` into
     accumulator `acc`, returning the result. `fresh?` true means `acc` is a
     new accumulator to fill and freeze (immutable first arg); false means
     `acc` is `a` itself, mutated in place. Internal helper for concat."
    (let [ta (type-of a)]
      (begin
        (fold (fn [_ b]
                (if (append-types-ok? ta (type-of b))
                  nil
                  (emit :error {:error :type-error
                                :message (string "concat: type mismatch — " ta
                                " vs " (type-of b))}))) nil rest)
        (if fresh? (push-all acc a) nil)
        (fold (fn [_ b] (push-all acc b)) nil rest)
        (if fresh? (freeze acc) acc)))))

(def concat
  (fn [a & rest]
    "Concatenate `a` with any number of additional collections of the same
     family, in one linear pass. A mutable first argument is extended in place
     and returned; an immutable first argument yields a fresh value of the same
     type. With no additional arguments, returns `a` unchanged."
    (if (empty? rest)
      a
      (match (type-of a)
        :array (concat-seq a rest (@array) true)
        :@array (concat-seq a rest a false)
        :string (concat-seq a rest (@string) true)
        :@string (concat-seq a rest a false)
        :bytes (concat-seq a rest (@bytes) true)
        :@bytes (concat-seq a rest a false)
        _ (reduce1 append (%pair a rest))))))

## ── Module export closure ──────────────────────────────────────────

(fn []
  {:last last
   :butlast butlast
   :append append
   :reverse reverse
   :fold fold
   :reduce reduce
   :concat concat})
