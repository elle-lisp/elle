(elle/epoch 18)
# audited: 2026-11-20
# std/check: what each generator makes from its choices, and the cases `enumerate` and `sample` give.
# docs/stdlib/check.md
#
# The counter-factual: an enumeration that walks its choices in any order
# gives the same set of cases, and a test of the set alone passes. Each
# assertion here names the cases in the order the document gives them.
# tests/lang/check-run.lisp holds `run`, `assert` and the shrinker.

(def check ((import "std/check")))

(defn simplest [g]
  "The one case a generator gives when every draw answers 0."
  (let [cases (check:enumerate g 0)]
    (assert (= 1 (length cases)) "depth 0 holds the one case of choice 0")
    (get cases 0)))

# ── the simplest value of each generator ──────────────────────────────
(assert (= :x (simplest (check:constant :x))) "constant: its value")
(assert (= false (simplest (check:bool))) "bool: false")
(assert (= 0 (simplest (check:int -5 5))) "int over 0: 0")
(assert (= 3 (simplest (check:int 3 7))) "int above 0: the low end")
(assert (= -3 (simplest (check:int -7 -3))) "int below 0: the high end")
(assert (= 4 (simplest (check:int 4 4))) "int of one value: that value")
(assert (= :a (simplest (check:element [:a :b :c]))) "element: the first")
(assert (= 1 (simplest (check:one-of [(check:constant 1) (check:constant 2)])))
        "one-of: the first generator's simplest")
(assert (= [] (simplest (check:array-of (check:bool) 3)))
        "array-of: the empty array")
(assert (= [false false] (simplest (check:array-of (check:bool) 3 :min 2)))
        "array-of with :min: that many of the simplest element")
(assert (= [0 false :x]
           (simplest (check:tuple (check:int 0 9) (check:bool)
                                  (check:constant :x))))
        "tuple: each part's simplest")
(assert (= 1 (simplest (check:map inc (check:int 0 9))))
        "map: f of the simplest")
(assert (= [:x :x]
           (simplest (check:bind (check:int 2 5)
                                 (fn [n]
                                   (check:array-of (check:constant :x) n :min n)))))
        "bind: the simplest of the generator f returns for the simplest v")

# ── enumerate: every choice sequence to a size, in lexicographic order ─
(assert (= [0 1 -1 2 -2] (check:enumerate (check:int -5 5) 4))
        "int draws nearer 0 first, a positive value before its negative")
(assert (= [0 1 -1 2 3 4 5] (check:enumerate (check:int -1 5) 10))
        "int runs on up the longer side of 0")
(assert (= [3 4 5] (check:enumerate (check:int 3 9) 2))
        "int above 0 counts up from its low end")
(assert (= [-3 -4 -5] (check:enumerate (check:int -9 -3) 2))
        "int below 0 counts down from its high end")
(assert (= [0 1 2 3] (check:enumerate (check:int 0 3) 10))
        "a draw never answers past its bound, so the enumeration ends")
(assert (= [false true] (check:enumerate (check:bool) 5)) "bool has two cases")
(assert (= [:a :b] (check:enumerate (check:element [:a :b :c]) 1))
        "element walks its array in order")
(assert (= [[0 0] [0 1] [0 2] [1 0] [1 1] [2 0]]
           (check:enumerate (check:tuple (check:int 0 9) (check:int 0 9)) 2))
        "a tuple's cases come in the lexicographic order of their choices")
(assert (= 20
           (length (check:enumerate (check:tuple (check:int 0 9) (check:int 0 9)
                                      (check:int 0 9)) 3)))
        "three draws that sum to 3 or less make 20 cases")
(assert (= [[] [false] [true] [false false]]
           (check:enumerate (check:array-of (check:bool) 3) 2))
        "array-of gives the shorter array first")
(assert (= [[] [0] [1] [0 0]]
           (check:enumerate (check:array-of (check:int 0 9) 2) 2))
        "and never an array past its max")
(assert (= [[false false] [false true] [true false] [false false false]]
           (check:enumerate (check:array-of (check:bool) 3 :min 2) 1))
        "array-of with :min starts at that length")
(assert (= [0 1 4 9]
           (check:enumerate (check:map (fn [n] (* n n)) (check:int 0 9)) 3))
        "map keeps the order of the generator it wraps")
(assert (= [0 1 false]
           (check:enumerate (check:one-of [(check:int 0 9) (check:bool)]) 1))
        "one-of counts the generator it picks as a choice")
(assert (= [[] [:x] [:x :x]]
           (check:enumerate (check:bind (check:int 0 2)
                                        (fn [n]
                                          (check:array-of (check:constant :x) n
                                            :min n))) 5))
        "bind feeds each value of g to f")

(def even (fn [draw] (* 2 (draw 50))))
(assert (= [0 2 4] (check:enumerate even 2))
        "a generator of your own calls draw directly")
(def pair-of-draws (fn [draw] [(draw 3) (draw 3)]))
(assert (= [[0 0] [0 1] [0 2] [1 0] [1 1] [1 2] [2 0] [2 1] [2 2]]
           (check:enumerate pair-of-draws 9))
        "draw answers from 0 to its bound less one")

# ── sample: random cases from a seed ──────────────────────────────────
(let [g (check:array-of (check:int -3 3) 6)
      a (check:sample g :seed 7 :count 40)
      b (check:sample g :seed 7 :count 40)]
  (assert (= 40 (length a)) "sample returns :count cases")
  (assert (= a b) "one seed gives the same cases twice")
  (each xs in a
    (assert (<= (length xs) 6) "a sampled array keeps to its max")
    (each x in xs
      (assert (and (<= -3 x) (<= x 3)) "a sampled integer keeps to its range"))))

(let [g (check:int 0 1000000)]
  (assert (not= (check:sample g :seed 1 :count 10)
                (check:sample g :seed 2 :count 10))
          "two seeds give different cases"))

(let [seen (apply set (check:sample (check:int 0 9) :seed 3 :count 300))]
  (assert (= 10 (length seen)) "300 samples of ten values reach every one"))

(let [top (- (bit/shl 1 62) 1)
      big (check:sample (check:int 0 top) :seed 5 :count 64)]
  (assert (all? (fn [n] (and (<= 0 n) (<= n top))) big)
          "a draw over 2^62 values keeps to its range")
  (assert (any? (fn [n] (> n (bit/shl 1 61))) big)
          "and reaches the upper half of it"))

# ── the submodules compose as the module does ─────────────────────────
(include-file "check/parts.lisp")
(assert (= [0 1 2] (run:enumerate (gen:int 0 9) 2))
        "the submodules, each passed to those that need it, enumerate")
(assert (= 17 (get (run:run (gen:int 0 99) (fn [n] (< n 17))) :value))
        "and shrink")
(assert (= (check:sample (check:int 0 99) :seed 9 :count 8)
           (run:sample (gen:int 0 99) :seed 9 :count 8))
        "and gives the cases the module gives")

# ── a draw reads its choice from bits ─────────────────────────────────
# The counter-factual: a replay that clamps a choice past its bound answers
# 2 for the bits 1 1 under a bound of 3, where a rejection reads on.
(defn draws [bits & bounds]
  "The choices a replay of BITS answers to draws under BOUNDS, in order."
  (let [draw ((source:replay bits) :draw)
        out @[]]
    (each n in bounds
      (push out (draw n)))
    (freeze out)))

(assert (= [2] (draws [1 0] 4)) "a draw reads the bits of its bound less one")
(assert (= [512] (draws [1 0 0 0 0 0 0 0 0 0] 1024))
        "the most significant first")
(assert (= [512] (draws [1] 1024)) "and every bit past the end is 0")
(assert (= [0] (draws [] 1001)) "so a draw past the end answers 0")
(assert (= [1] (draws [1 1 0 1] 3))
        "a draw rejects bits that name its bound or more, and reads again")
(assert (= [0] (draws [1 1 1 1 1 1 1 0 0 1] 1001))
        "and reads zeros once the stream ends")
(assert (= [0 1] (draws [1] 1 2)) "a draw of bound 1 reads no bit")
(assert (= [1 2 3] (draws [0 1 1 0 1 1] 4 4 4))
        "each draw reads on from the last")

(println "check: all tests passed")
