(elle/epoch 14)
# audited: 2026-09-30
# each binds every element of a collection in turn, and the collection's kind decides what an element is.
# docs/loops.md

(defn collect [coll]
  (let [out @[]]
    (each x in coll
      (push out x))
    (freeze out)))

# ── The form ─────────────────────────────────────────────────────────

(let [out @[]]
  (each x [1 2 3]
    (push out x))
  (assert (= (freeze out) [1 2 3]) "each: in is optional"))

## The template binds the collection once. The counter-factual: a template
## that names the collection expression in each arm runs it twice or more.
(let [@calls 0]
  (each x in (begin
               (assign calls (+ calls 1))
               [1 2 3])
    x)
  (assert (= calls 1) "each: the collection expression runs once"))

## Run a loop to its end, and answer the loop's value.
(defn run-through [coll]
  (each x in coll
    x))

(assert (nil? (run-through [1 2])) "each: an array loop returns nil")
(assert (nil? (run-through (list 1 2))) "each: a list loop returns nil")
(assert (nil? (run-through {:a 1})) "each: a struct loop returns nil")

(defn sum-of [coll]
  (var total 0)
  (each x in coll
    (assign total (+ total x)))
  total)

(assert (= (sum-of [1 2 3]) 6) "each: assign reaches out of an array loop")
(assert (= (sum-of |1 2 3|) 6) "each: assign reaches out of a set loop")

(let [out @[]]
  (each x in [1 2]
    (each y in [:a :b]
      (push out [x y])))
  (assert (= (freeze out) [[1 :a] [1 :b] [2 :a] [2 :b]])
          "each: nested loops visit every pair"))

# ── Lists, arrays, strings, bytes ────────────────────────────────────

(assert (= (collect (list 1 2 3)) [1 2 3]) "each: a list, in order")
(assert (= (collect ()) []) "each: the empty list makes no pass")
(assert (= (collect [:a :b]) [:a :b]) "each: an array, in order")
(assert (= (collect @[:a :b]) [:a :b]) "each: an @array, in order")
(assert (= (collect []) []) "each: the empty array makes no pass")

## A string's elements are grapheme clusters, not bytes or code points: the
## counter-factual splits the emoji and its skin-tone modifier apart.
(assert (= (collect "a👋🏽b") ["a" "👋🏽" "b"])
        "each: a string's graphemes")
(assert (= (collect @"ab") ["a" "b"]) "each: an @string's graphemes")

(assert (= (collect (bytes 1 2 255)) [1 2 255]) "each: bytes as integers")
(assert (= (collect (@bytes 1 2 255)) [1 2 255]) "each: @bytes as integers")

# ── Sets and structs ─────────────────────────────────────────────────

(assert (= (sort (collect |3 1 2|)) [1 2 3]) "each: a set's members")
(assert (= (sort (collect @|3 1 2|)) [1 2 3]) "each: an @set's members")

## A struct's elements are its [key value] pairs, in the order `pairs`
## gives. The counter-factual: a loop that bound the keys alone, or the
## values alone, still makes one pass per entry.
(let [s {:b 2 :a 1 :c 3}]
  (assert (= (collect s) (->array (pairs s)))
          "each: a struct's pairs, in pairs order"))

(let [s @{:b 2 :a 1}]
  (assert (= (collect s) (->array (pairs s)))
          "each: an @struct's pairs, in pairs order"))

(let [ks @[]
      vs @[]]
  (each [k v] in {:a 1}
    (push ks k)
    (push vs v))
  (assert (= [(freeze ks) (freeze vs)] [[:a] [1]])
          "each: a struct pair destructures into key and value"))

# ── Fibers ───────────────────────────────────────────────────────────

(defn generator [xs result]
  (fiber/new (fn []
               (each x in xs
                 (yield x))
               result) |:yield|))

(assert (= (collect (generator [1 2 3] nil)) [1 2 3])
        "each: a fiber's yields, in order")

## The counter-factual: a loop that ends on the first falsy yield stops
## after 1, and reads nil and false as the end of the fiber.
(assert (= (collect (generator [1 nil false 4] nil)) [1 nil false 4])
        "each: a fiber's nil and false yields are elements")

## The counter-factual: a loop that binds every value a resume answers
## binds the return value :done, then resumes the dead fiber and raises.
(assert (= (collect (generator [1 2] :done)) [1 2])
        "each: a fiber's return value is not an element")

(assert (= (collect (generator [] :done)) [])
        "each: a fiber that yields nothing makes no pass")

## The counter-factual: a loop that ends in a while returns nil, and the
## fiber's return value is lost.
(assert (= (run-through (generator [1 2] :done)) :done)
        "each: a fiber loop returns the value the fiber returns")
(assert (= (run-through (generator [] :empty)) :empty)
        "each: a fiber that yields nothing still returns its value")

## Loop over a fiber that yields 1 and then raises :boom. Answer whether
## the loop finished, the error it raised, and the elements it bound.
(defn each-failing [mask]
  (let [fib (fiber/new (fn []
                         (yield 1)
                         (error {:error :boom :message "boom"})
                         (yield 2)) mask)
        out @[]
        [ok? err] (protect (each x in fib
                             (push out x)))]
    [ok? (get err :error) (freeze out)]))

(assert (= (each-failing |:yield|) [false :boom [1]])
        "each: an error inside the fiber propagates")

## The trap: a fiber whose mask catches errors does not raise from
## fiber/resume. It suspends holding the error and reports :paused, as a
## yield does. The counter-factual binds the error struct as an element.
(assert (= (each-failing |:yield :error|) [false :boom [1]])
        "each: an error a fiber's mask catches still propagates")

(let [done (generator [] nil)]
  (fiber/resume done)
  (let [[_ direct] (protect (fiber/resume done))
        [ok? err] (protect (run-through done))]
    (assert (= [ok? (get err :error)] [false (get direct :error)])
            "each: a completed fiber raises as fiber/resume does")))

# ── :iter ────────────────────────────────────────────────────────────

## A struct whose traits carry :iter iterates as its protocol says. The
## counter-factual walks the struct's own pairs.
(def countdown
  (with-traits {:from 3}
               @{:Sequence {:iter (fn [self] (generator [3 2 1] nil))}}))

(assert (= (collect countdown) [3 2 1]) "each: a value's :iter yields")

# ── Early exit ───────────────────────────────────────────────────────

## Break out of a loop at the first element equal to TARGET, carrying it.
(defn break-at [coll target]
  (each x in coll
    (when (= x target) (break x))))

(assert (= (break-at [1 2 3] 2) 2)
        "each: break's value is the array loop's value")
(assert (= (break-at (list 1 2 3) 2) 2)
        "each: break's value is the list loop's value")
(assert (= (break-at {:a 1 :b 2} [:b 2]) [:b 2])
        "each: break's value is the struct loop's value")
(assert (= (break-at (generator [5 6] nil) 6) 6)
        "each: break's value is the fiber loop's value")

## The counter-factual: a loop that answers the fiber's value after its
## while answers 5, the value the fiber last yielded.
(assert (= (each x in (generator [5 6] :done)
             (break 99)) 99)
        "each: a break before the fiber completes returns the break's value")

## each resumes once per pass rather than draining the fiber first, so a
## break ends a loop over a fiber that never completes.
(let [naturals (fiber/new (fn []
                            (var n 0)
                            (forever
                              (yield n)
                              (assign n (+ n 1)))) |:yield|)]
  (assert (= (break-at naturals 3) 3)
          "each: a break ends a loop over an endless fiber"))

(let [out @[]]
  (each x in [1 2 3 4]
    (push out x)
    (when (= x 2) (break)))
  (assert (= (freeze out) [1 2]) "each: break skips the remaining elements"))

# ── Not a sequence ───────────────────────────────────────────────────

(defn each-error [coll]
  (let [[ok? err] (protect (run-through coll))]
    (if ok? :no-error (get err :error))))

(assert (= (each-error 42) :type-error) "each: an integer raises")
(assert (= (each-error :kw) :type-error) "each: a keyword raises")
(assert (= (each-error nil) :type-error) "each: nil raises")
