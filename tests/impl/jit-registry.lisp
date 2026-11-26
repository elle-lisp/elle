(elle/epoch 18)
# audited: 2026-11-26
# Each compiled function's entry address and label are in the JIT code-address registry.
# docs/impl/jit-map.md
#
# A label is the declared name and the origin, so each expected label is built
# from the probe's `meta/origin`. A build without the JIT tier refuses
# `compile/run-on :jit`, compiles nothing, and checks only the queries.

(defn place [f]
  "Where f was written, as file:line:col."
  (let [o (meta/origin f)]
    (string (get o :file) ":" (get o :line) ":" (get o :col))))

(defn labeled? [label]
  "Whether the map holds an entry whose whole label is label."
  (any? (fn [line] (string/ends-with? line (string " " label)))
        (string/split (vm/query "jit/map" nil) "\n")))

# A forced compile, which answers whether this build compiles at all.
(defn registry-probe [x]
  (if x x 1))
(def anonymous (fn [x] (+ x 1)))
(def compiles? (first (protect (compile/run-on :jit registry-probe 1))))

(when compiles?
  (assert (labeled? (string "registry-probe@" (place registry-probe)))
          "jit/map: a named function's label is its name, @, and its origin")
  (compile/run-on :jit anonymous 1)
  (assert (labeled? (place anonymous))
          "jit/map: a function with no name is labeled with its origin alone"))

# Two helpers bound to one name, each in its own letrec. Each recurses, so its
# calls make it hot and the build's own compile path labels it. The
# counter-factual is the bare name: both would be one `go`, and perf would
# merge their samples into one frame.
(defn make-down []
  (letrec [go (fn [k] (if (= k 0) 0 (go (- k 1))))]
    go))
(defn make-up []
  (letrec [go (fn [k]
                (if (= k 0)
                  0
                  (+ 1 (go (- k 1)))))]
    go))
(def down (make-down))
(def up (make-up))

(def @i 0)
(while (< i 50)
  (down 3)
  (up 3)
  (assign i (+ i 1)))
# Drain pending background compilations so a compile that was still in
# flight lands before the map is read.
(jit/rejections)
(down 0)
(up 0)

(when compiles?
  (assert (and (vm/query "jit?" down) (vm/query "jit?" up))
          "jit?: both recursive helpers compiled")
  (assert (not (= (place down) (place up)))
          "meta/origin: the two helpers were written in two places")
  (assert (labeled? (string "go@" (place down)))
          "jit/map: the first go is labeled with its own origin")
  (assert (labeled? (string "go@" (place up)))
          "jit/map: the second go is labeled with its own origin"))

# Each line of the peek window names its own address, and an address outside
# every registered block answers nil rather than faulting.
(let [m (vm/query "jit/map" nil)]
  (assert (string? m) "jit/map: renders as a string")
  (when (> (length m) 0)
    (let* [line (get (string/split m "\n") 0)
           addr (get (string/split line " ") 0)
           words (vm/query "jit/peek" addr)]
      (assert (string? words) "jit/peek: a registered entry answers words")
      (assert (string/contains? words "0x") "jit/peek: words render as hex")
      (assert (> (length (string/split words "\n")) 1)
              "jit/peek: the window spans several address-prefixed lines")
      (each l in (string/split words "\n")
        (assert (string/contains? l ":")
                "jit/peek: each window line names its address"))))
  (assert (nil? (vm/query "jit/peek" "0x10"))
          "jit/peek: an address below every entry answers nil")
  (assert (nil? (vm/query "jit/peek" "junk"))
          "jit/peek: a malformed address answers nil"))

(println "ok: jit code-address registry answers")
