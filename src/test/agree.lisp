(elle/epoch 18)
# audited: 2026-11-19
## elle test — when two tiers' values agree, and how a `diverge` row renders them.
## docs/test-runner.md
## docs/types.md
##
## A fragment of one module (see store.lisp).

# The types `=` compares by content. Every other type is a reference type:
# `=` compares its values by identity, and each tier builds its own.
(def data-types
  |:nil :boolean :integer :float :symbol :keyword :native-fn :list :array
   :@array :struct :@struct :string :@string :bytes :@bytes :set :@set :box
   :library-handle :ffi-signature :ffi-type|)

(defn agree-kind [v]
  "The kind V agrees by: its type, with an @array taken as an array and an
   @struct as a struct, as `=` takes them."
  (let [t (type-of v)]
    (cond
      (= t :@array) :array
      (= t :@struct) :struct
      t)))

# Each container helper takes the element test as SAME?, so it reads no name
# defined after it, and `agree?` hands itself down.
(defn array-agree? [xs ys same?]
  "Do arrays XS and YS hold one count of values that agree pair by pair?"
  (var i 0)
  (var ok (= (length xs) (length ys)))
  (while (and ok (< i (length xs)))
    (assign ok (same? (get xs i) (get ys i)))
    (assign i (+ i 1)))
  ok)

(defn list-agree? [xs ys same?]
  "Do lists XS and YS agree pair by pair, their tails included?"
  (if (and (pair? xs) (pair? ys))
    (and (same? (first xs) (first ys)) (list-agree? (rest xs) (rest ys) same?))
    (same? xs ys)))

(defn struct-agree? [a b same?]
  "Do structs A and B hold the same keys, with values that agree key by key?"
  (var ok (= (length (keys a)) (length (keys b))))
  (each k in (keys a)
    (when ok
      (assign ok (and (has? b k) (same? (get a k) (get b k))))))
  ok)

(defn agree? [a b]
  "Does A, one tier's value, agree with B, another tier's?"
  (if (= a b)
    true
    (let [kind (agree-kind a)]
      (cond
        (not (= kind (agree-kind b))) false
        (= kind :array) (array-agree? a b agree?)
        (= kind :list) (list-agree? a b agree?)
        (= kind :struct) (struct-agree? a b agree?)
        (not (contains? data-types kind))))))

(defn all-agree? [x xs]
  "Does every value in XS agree with X? A divergence is a value that does not."
  (if (empty? xs)
    true
    (and (agree? x (first xs)) (all-agree? x (rest xs)))))

(defn render-pairs [pairs]
  "The passing tiers' values, as the reason of the synthetic diverge row."
  (if (empty? pairs)
    ""
    (let [p (first pairs)
          one (string (get p 0) "=" (string (get p 1)))]
      (if (empty? (rest pairs))
        one
        (string one " " (render-pairs (rest pairs)))))))
