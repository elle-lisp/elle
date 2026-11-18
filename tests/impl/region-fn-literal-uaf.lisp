(elle/epoch 15)
# audited: 2026-11-16
# A constant a function reads keeps its value on every call, however the function returns, stores, reassigns or yields it.
# docs/impl/hoist.md
#
# The sidecar arms guardfree and keeps the JIT off, so a constant freed while
# a function still holds it faults at the next call's read. The value faces
# of these shapes live in tests/lang/literal-fresh.lisp and
# tests/lang/literal-var.lisp, where a freed constant reads correctly until
# its memory is reused.
#
# The counter-factual: the unit builds each constant once, and a closure holds
# it in its environment. A release that treated the first caller's copy, or a
# var's first assign, or one closure's demise as the constant's last use
# would free the one value every later call reads.

# ── returned and stored ─────────────────────────────────────────────

(defn tag []
  "tag")
(defn row []
  ["tag" '(1 "q") {:k "v"} |"s"| b[1 2]])

(defn kept [n make]
  "An array of the n values (make) answers."
  (def out @[])
  (repeat n (push out (make)))
  (freeze out))

(assert (= (kept 3 tag) ["tag" "tag" "tag"])
        "a returned string outlives each call")
(assert (= (kept 3 tag) ["tag" "tag" "tag"]) "and the next array's copies too")
(assert (= (kept 2 row) [(row) (row)])
        "a returned collection outlives each call")

(defn stamp [out]
  (push out "stamp")
  out)
(def stamps @[])
(stamp stamps)
(stamp stamps)
(assert (= (freeze stamps) ["stamp" "stamp"])
        "a pushed string outlives its call")

# ── reassigned ──────────────────────────────────────────────────────

(defn suffixed [i]
  "The literal \"x\" with i appended once."
  (var s "x")
  (assign s (string s i))
  s)

(defn grown [i]
  "The literal [0] with i appended once."
  (var a [0])
  (assign a (append a [i]))
  a)

(assert (= (map suffixed [0 1 2]) ["x0" "x1" "x2"])
        "each call starts from the string")
(assert (= (map grown [0 1 2]) [[0 0] [0 1] [0 2]])
        "each call starts from the array")

# ── held by many closures ───────────────────────────────────────────

(defn greeters [n]
  "n closures, built in a loop, each answering the one literal."
  (def out @[])
  (repeat n (push out (fn [] "hello")))
  (freeze out))

(defn answers [fs]
  (map (fn [f] (f)) fs))

(assert (= (answers (greeters 3)) ["hello" "hello" "hello"])
        "each closure reads it")
(assert (= (answers (greeters 2)) ["hello" "hello"])
        "after the first ones are freed")

# ── read by mutually recursive functions ────────────────────────────

(defn even-tag [n]
  (if (= n 0) "even" (odd-tag (- n 1))))
(defn odd-tag [n]
  (if (= n 0) "odd" (even-tag (- n 1))))

(assert (= (map even-tag [0 1 2 3]) ["even" "odd" "even" "odd"])
        "a file's mutually recursive functions read their strings")

(defn parity [n]
  "even or odd, by two local functions that call each other."
  (defn ev [k]
    (if (= k 0) "even" (od (- k 1))))
  (defn od [k]
    (if (= k 0) "odd" (ev (- k 1))))
  (ev n))

(assert (= (map parity [0 1 2 3]) ["even" "odd" "even" "odd"])
        "local mutually recursive functions read their strings")

# ── yielded ─────────────────────────────────────────────────────────

(defn ticks [n]
  "n resumes of a generator that yields one literal."
  (def gen (fiber/new (fn [] (forever (yield "tick"))) |:yield|))
  (def out @[])
  (repeat n (push out (fiber/resume gen)))
  (freeze out))

(assert (= (ticks 3) ["tick" "tick" "tick"])
        "a yielded string outlives each resume")
(assert (= (ticks 2) ["tick" "tick"]) "and a later generator's")

# ── read by a macro ─────────────────────────────────────────────────

(defmacro labeled (x)
  `(string "label-" ,x))

(defn labels []
  "Two expansions of one macro, whose transformer reads one template."
  [(labeled "a") (labeled "b")])

(defn more-labels []
  [(labeled "c")])

(assert (= (labels) ["label-a" "label-b"])
        "each expansion builds from the template's constants")
(assert (= (labels) ["label-a" "label-b"]) "and so does the next")
(assert (= (more-labels) ["label-c"]) "and an expansion in another function")

(println "region-fn-literal-uaf: ok")
