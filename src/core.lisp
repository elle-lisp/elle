(elle/epoch 12)
## audited: 2026-09-20
## Pre-prelude definitions: the sequence operators, and the trait layer the
## operators dispatch through.
## docs/traits.md
##
## Compiled and executed before the prelude loads.
## Only raw special forms and %-prefixed primitives are available.
## Provides functions that prelude macros need at expansion time.

## ── Trait dispatch for the collection operators ────────────────────
## The access primitives (first/rest/length/empty?/has?) resolve through the
## trait table inside the VM, and `trait/op` and `trait/iterable?` (primitives,
## src/primitives/traits.rs) read that table for the Elle-level operators.
## These two complete the layer: an iterator drains into an array, and an array
## builds a collection back. A method named for an operator overrides that
## operator outright; :iter together with :Collection :empty/:conj drives every
## operator that has no method of its own. docs/traits.md holds the protocol,
## the method names, and the order a method's arguments arrive in.

(def iter-failed?
  (fn [fib]
    "True when an iterator fiber stopped on an error. An error does not unwind
     a fiber: it suspends holding the error signal and still reports :paused,
     so the status alone reads a failure as a pause. Internal helper."
    (if (%eq (fiber/status fib) :error)
      true
      (%not (%eq 0 (bit/and (fiber/bits fib) 1))))))

(def trait/elements
  (fn [coll]
    "Return COLL's elements as an immutable array. A value whose traits carry
     :iter is drained through that iterator; every other value converts with
     ->array, which is what a builtin container and a plain set or struct do.
     An iterator that raises propagates its error to the caller."
    (if (trait/iterable? coll)
      (let [fib ((trait/op coll :iter) coll)
            acc (@array)]
        (letrec [drain (fn []
                         (begin
                           (fiber/resume fib)
                           (if (iter-failed? fib)
                             (fiber/propagate fib)
                             (if (%eq (fiber/status fib) :dead)
                               (freeze acc)
                               (begin
                                 (core-push acc (fiber/value fib))
                                 (drain))))))]
          (drain)))
      (->array coll))))

(def trait/rebuild
  (fn [coll items]
    "Build a collection like COLL out of the array ITEMS: seed with COLL's
     :empty trait method, then add each element with its :conj method, in
     order. Signals :type-error when COLL's traits carry no such pair."
    (let [mk (trait/op coll :empty)
          cj (trait/op coll :conj)]
      (if (if mk cj false)
        (let [n (length items)]
          (letrec [go (fn [i acc]
                        (if (%lt i n)
                          (go (%add i 1) (cj acc (get items i)))
                          acc))]
            (go 0 (mk coll))))
        (emit :error {:error :type-error
                      :reason :not-a-collection
                      :message "rebuild: no :empty and :conj trait methods"})))))

(def last
  (fn [coll]
    "Return the last element of a sequence. Signals :argument-error if the
     sequence is empty. COLL's :last trait method answers instead when it
     carries one."
    (let [m (trait/op coll :last)]
      (if m
        (m coll)
        (let [xs (if (trait/iterable? coll) (trait/elements coll) coll)]
          (if (%eq (length xs) 0)
            (emit :error {:error :argument-error :message "last: empty sequence"})
            (get xs (%sub (length xs) 1))))))))

(def butlast
  (fn [coll]
    "Return a new sequence with the last element removed. An empty sequence
     yields an empty slice. COLL's :butlast trait method answers instead when
     it carries one; otherwise a trait-driven collection is rebuilt from its
     elements."
    (let [m (trait/op coll :butlast)]
      (if m
        (m coll)
        (if (trait/iterable? coll)
          (let [xs (trait/elements coll)
                n (length xs)]
            (trait/rebuild coll (if (%eq n 0) xs (slice xs 0 (%sub n 1)))))
          (let [n (length coll)]
            (if (%eq n 0) (slice coll 0 0) (slice coll 0 (%sub n 1)))))))))

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

(def reverse-builtin
  (fn [coll]
    "Reverse a builtin sequence: list/syntax yield a new list, and the other
     families a new immutable value of their own kind. Signals :type-error
     for anything else. Internal helper for reverse."
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

(def reverse
  (fn [coll]
    "Return a reversed copy of a sequence (list, array, string, or bytes).
     Lists/syntax return a new list; other sequences return a new immutable
     value of the same family. COLL's :reverse trait method answers instead
     when it carries one, and a trait-driven collection is rebuilt from its
     elements. Signals :type-error for a non-sequence."
    (let [m (trait/op coll :reverse)]
      (if m
        (m coll)
        (if (trait/iterable? coll)
          (trait/rebuild coll (reverse-builtin (trait/elements coll)))
          (reverse-builtin coll))))))

## ── fold / reduce ──────────────────────────────────────────────────

## Normalize once with `trait/elements`, then walk by INDEX — never (first/rest)
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
     collection. `f` is called as (f acc element). COLL's :fold trait method
     answers instead when it carries one, and a trait-driven collection folds
     over the elements its :iter method yields."
    (let [m (trait/op coll :fold)]
      (if m
        (m coll f init)
        (let [arr (trait/elements coll)
              n (length arr)]
          (core-fold-step f arr n 0 init))))))

(def reduce fold)

(def reduce1
  (fn [f coll]
    "Left-fold `f` over `coll` using its first element as the seed:
     (f (f e0 e1) e2)…. Signals :argument-error on an empty collection.
     Internal helper (the user-facing reduce/reduce1 live in stdlib)."
    ## Index walk from element 1, seeded by element 0 — never (rest arr), which
    ## would mint a throwaway tail slice (the same F1a scratch fold avoids above).
    (let [arr (trait/elements coll)
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
   :concat concat
   :trait/elements trait/elements
   :trait/rebuild trait/rebuild})
