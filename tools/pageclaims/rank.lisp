(elle/epoch 13)
# audited: 2026-09-29
# Ranks the code paths that claim region pages, from a callgrind file.
# docs/impl/region/colocation.md
#
# The file comes from a run with `--separate-callers12=*add_page*`, so each
# caller context of `RegionPool::add_page` is a function of its own, named
# `add_page'caller'caller'…`. A `calls=N` line after a `cfn=` naming such a
# context is N page claims through that context.

(def add-page "<elle::value::fiberheap::regionpool::RegionPool>::add_page")

# The frames that only move an allocation toward a page: the region store, the
# page pool and the heap's entry points (all under `fiberheap`), the `Alloc`
# capability, and the value builders. A path starts above them.
(def plumbing
  ["fiberheap" "arena::alloc" "alloc_in_region" "alloc_obj" "alloc_region_slice"
   "ctx::Alloc" "value::build" "<elle::value::heap"])

(defn plumbing? [frame]
  (any? (fn [p] (string/contains? frame p)) plumbing))

(defn claims-page? [callee]
  "Whether CALLEE, a function name, is add_page or one of its caller contexts."
  (and (not (nil? callee))
       (or (= callee add-page)
           (string/starts-with? callee (string add-page "'")))))

(defn path-of [context depth]
  "The first DEPTH frames of CONTEXT above add_page that are not plumbing,
  innermost first, without the `elle::` prefix."
  (def frames (string/split context "'"))
  (def kept @[])
  (var i 1)
  (while (and (< i (length frames)) (< (length kept) depth))
    (def frame (get frames i))
    (unless (plumbing? frame) (push kept (string/replace frame "elle::" "")))
    (assign i (+ i 1)))
  (string/join kept " <- "))

(defn path-for [paths context depth]
  "CONTEXT's path, computed once and kept in PATHS."
  (when (nil? (get paths context)) (put paths context (path-of context depth)))
  (get paths context))

(defn name-line [names line open]
  "Read a `fn=(ID) NAME` or `cfn=(ID) NAME` line whose `(` is at OPEN, and
  answer the function it names. The first line for an ID carries the name, and
  NAMES keeps it for the later lines that carry the ID alone."
  (def close (string/find line ")" open))
  (def id (slice line (+ open 1) close))
  (when (< (+ close 1) (length line))
    (put names id (slice line (+ close 2) (length line))))
  (get names id))

(defn rank [read-line depth]
  "Sum the page claims of each call path in the callgrind file READ-LINE
  yields, a line per call and then nil. A path is DEPTH frames deep. Answers
  {:total claims :paths [[claims path] …]}, the most claims first, and a tie
  ordered by the path's text."
  (def names @{})
  (def paths @{})
  (def claims @{})
  (var total 0)
  (var callee nil)
  (forever
    (def line (read-line))
    (when (nil? line) (break nil))
    (cond
      (string/starts-with? line "cfn=(") (assign callee (name-line names line 4))
      (string/starts-with? line "fn=(") (name-line names line 3)
      (and (string/starts-with? line "calls=") (claims-page? callee))
        (let [n (parse-int (get (string/split (slice line 6 (length line)) " ")
                                0))
              path (path-for paths callee depth)]
          (assign total (+ total n))
          (put claims path (+ n (get claims path 0))))))
  (def ranked (sort (->array (map (fn [[path n]] [(- n) path]) (pairs claims)))))
  {:total total :paths (->array (map (fn [[less path]] [(- less) path]) ranked))})

(fn [] {:rank rank})
