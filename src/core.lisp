(elle/epoch 11)
## Pre-prelude definitions
##
## Compiled and executed before the prelude loads.
## Only raw special forms and %-prefixed primitives are available.
## Provides functions that prelude macros need at expansion time.

(def last
  (fn [coll]
    (if (%eq (length coll) 0)
      (emit :error {:error :argument-error :message "last: empty sequence"})
      (get coll (%sub (length coll) 1)))))

(def butlast
  (fn [coll]
    (let [n (length coll)]
      (if (%eq n 0) (slice coll 0 0) (slice coll 0 (%sub n 1))))))

## ── Helpers (not exported) ─────────────────────────────────────────

(def push-all
  (fn [dst src]
    (letrec [go (fn [xs]
                  (if (empty? xs)
                    dst
                    (begin
                      (push dst (first xs))
                      (go (rest xs)))))]
      (go (->array src)))))

(def merge-into
  (fn [dst src]
    (letrec [go (fn [ks]
                  (if (empty? ks)
                    dst
                    (begin
                      (put dst (first ks) (get src (first ks)))
                      (go (rest ks)))))]
      (go (keys src)))))

## ── reverse ────────────────────────────────────────────────────────

(def reverse
  (fn [coll]
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
                            (push r (get coll i))
                            (go (%sub i 1)))))]
            (go (%sub n 1)))
          (match t
            :array (freeze r)
            :string (freeze r)
            :bytes (freeze r)
            _ r))))))

## ── fold / reduce ──────────────────────────────────────────────────

(def fold
  (fn [f init coll]
    (letrec [go (fn [acc xs]
                  (if (empty? xs)
                    acc
                    (go (f acc (first xs)) (rest xs))))]
      (go init (->array coll)))))

(def reduce fold)

(def reduce1
  (fn [f coll]
    (let [arr (->array coll)]
      (if (empty? arr)
        (emit :error {:error :argument-error
                      :message "reduce1: empty collection"})
        (fold f (first arr) (rest arr))))))

## ── append ─────────────────────────────────────────────────────────
## :syntax branch handles syntax lists from quasiquote expansion.

(def append-list
  (fn [a b]
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
    (let [ta (type-of a)]
      (if (%not (append-types-ok? ta (type-of b)))
        (emit :error {:error :type-error
                      :message (string "append: type mismatch — " ta " vs "
                                       (type-of b))})
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
          :@set (union a b)
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

(def concat (fn [a & rest] (if (empty? rest) a (reduce1 append (%pair a rest)))))

## ── Module export closure ──────────────────────────────────────────

(fn []
  {:last last
   :butlast butlast
   :append append
   :reverse reverse
   :fold fold
   :reduce reduce
   :concat concat})
