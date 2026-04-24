(elle/epoch 9)
## Elle standard library
##
## Loaded at startup after primitives are registered.
## Unlike the prelude (which is macro-only), these define
## runtime functions that need the full pipeline.
##
## Exported functions:
## - Higher-order: map, filter, fold, reduce, keep
## - Combinators: identity, complement, constantly, compose, comp, partial, juxt
## - Predicates: all?, any?, some, none?
## - Search: find, find-index, index-of, last-index-of
## - Transformation: flatten, group-by, partition, take-while, drop-while
## - Struct operations: merge
## - Stream sinks: stream/for-each, stream/fold, stream/collect, stream/into-array
## - Stream transforms: stream/map, stream/filter, stream/take, stream/drop, stream/concat, stream/zip, stream/pipe
## - Stream ports: port/lines, port/chunks, port/writer
## - Subprocess convenience: subprocess/system

## ── Higher-order functions ──────────────────────────────────────────

(defn map [f coll]
  "Apply f to each element of coll, returning a new collection of the same type. Type-preserving: lists return lists, arrays return arrays, sets return sets."
  (cond
    (or (array? coll) (string? coll) (bytes? coll))
     (let* [len (length coll)
            acc @[]]
       (def @i 0)
       (while (< i len)
         (push acc (f (get coll i)))
         (assign i (+ i 1)))
       (if (mutable? coll) acc (freeze acc)))
    (set? coll)
     (let* [items (set->array coll)
            acc @||]
       (each item in items
         (add acc (f item)))
       (if (mutable? coll) acc (freeze acc)))
    (or (pair? coll) (empty? coll))
     (if (empty? coll) ()
       (cons (f (first coll)) (map f (rest coll))))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn filter [p coll]
  "Return elements of coll for which (p element) is truthy. Type-preserving."
  (cond
    (or (array? coll) (string? coll) (bytes? coll))
     (let* [len (length coll)
            acc @[]]
       (def @i 0)
       (while (< i len)
         (let [item (get coll i)]
           (when (p item) (push acc item)))
         (assign i (+ i 1)))
       (if (mutable? coll) acc (freeze acc)))
    (set? coll)
     (let* [items (set->array coll)
            acc (if (mutable? coll) (@set) (set))]
       (each item in items
         (when (p item) (add acc item)))
       acc)
    (or (pair? coll) (empty? coll))
     (if (empty? coll) ()
       (if (p (first coll))
         (cons (first coll) (filter p (rest coll)))
         (filter p (rest coll))))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn fold [f init lst]
  "Reduce lst by applying (f accumulator element) left to right, starting from init. Alias: reduce."
  (if (empty? lst)
    init
    (fold f (f init (first lst)) (rest lst))))

(def reduce fold)
(def keep filter)

## ── Functional combinators ──────────────────────────────────────────

(defn identity [x] x)

(defn complement [f]
  (fn (& args) (not (f ;args))))

(defn constantly [x]
  (fn (& _) x))

(defn compose [& fns]
  (fold (fn (composed f)
          (fn (& args) (composed (f ;args))))
        identity
        fns))

(def comp compose)

(defn partial [f & bound]
  (fn (& args) (f ;bound ;args)))

(defn juxt [& fns]
  (fn (& args)
    (map (fn (f) (f ;args)) fns)))

## ── Collection search & predicates ──────────────────────────────────

(defn all? [pred coll]
  (cond
    (or (pair? coll) (empty? coll))
     (if (empty? coll)
       true
       (if (pred (first coll))
         (all? pred (rest coll))
         false))
    (or (array? coll) (array? coll))
     (letrec [loop (fn (i)
                      (if (>= i (length coll))
                        true
                        (if (pred (get coll i))
                          (loop (+ i 1))
                          false)))]
       (loop 0))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn any? [pred coll]
  (cond
    (or (pair? coll) (empty? coll))
     (if (empty? coll)
       false
       (if (pred (first coll))
         true
         (any? pred (rest coll))))
    (or (array? coll) (array? coll))
     (letrec [loop (fn (i)
                      (if (>= i (length coll))
                        false
                        (if (pred (get coll i))
                          true
                          (loop (+ i 1)))))]
       (loop 0))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn find [pred coll]
  (cond
    (or (pair? coll) (empty? coll))
     (if (empty? coll)
       nil
       (if (pred (first coll))
         (first coll)
         (find pred (rest coll))))
    (or (array? coll) (array? coll))
     (letrec [loop (fn (i)
                      (if (>= i (length coll))
                        nil
                        (if (pred (get coll i))
                          (get coll i)
                          (loop (+ i 1)))))]
       (loop 0))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn find-index [pred coll]
  (cond
    (or (pair? coll) (empty? coll))
     (letrec [go (fn (i l)
                    (if (empty? l)
                      nil
                      (if (pred (first l))
                        i
                        (go (+ i 1) (rest l)))))]
       (go 0 coll))
    (or (array? coll) (array? coll))
     (letrec [loop (fn (i)
                      (if (>= i (length coll))
                        nil
                        (if (pred (get coll i))
                          i
                          (loop (+ i 1)))))]
       (loop 0))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn count [pred coll]
  (cond
    (or (pair? coll) (empty? coll))
     (fold (fn (n x) (if (pred x) (+ n 1) n)) 0 coll)
    (or (array? coll) (array? coll))
     (letrec [loop (fn (i n)
                      (if (>= i (length coll))
                        n
                        (loop (+ i 1) (if (pred (get coll i)) (+ n 1) n))))]
       (loop 0 0))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn nth [n coll]
  (get coll n))

## ── Collection transforms ───────────────────────────────────────────

(defn zip [& colls]
  "Zip collections element-wise into a collection of lists. Stops at the shortest input."
  (letrec
    [to-list (fn (c)
       (cond
         (or (pair? c) (empty? c)) c
         (or (array? c) (array? c))
          (letrec [loop (fn (i acc)
                           (if (>= i (length c))
                             (reverse acc)
                             (loop (+ i 1) (cons (get c i) acc))))]
            (loop 0 ()))
         true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))
     from-list (fn (lst orig)
       (cond
         (or (pair? orig) (empty? orig)) lst
         (array? orig)
          (let [arr @[]]
            (each x in lst (push arr x))
            arr)
         (array? orig) (apply array lst)))
     zip-lists (fn (lists)
       (if (any? empty? lists)
         ()
         (cons (map first lists)
               (zip-lists (map rest lists)))))]
    (if (empty? colls)
      ()
      (let* [lists (map to-list colls)
             result (zip-lists lists)]
        (from-list result (first colls))))))

(defn flatten [coll]
  (letrec
    [to-list (fn (c)
       (letrec [loop (fn (i acc)
                        (if (>= i (length c))
                          (reverse acc)
                          (loop (+ i 1) (cons (get c i) acc))))]
         (loop 0 ())))
     flat (fn (lst)
       (if (empty? lst)
         ()
         (let [x (first lst)]
           (cond
             (pair? x)
              (append (flat x) (flat (rest lst)))
             (or (array? x) (array? x))
              (append (flat (to-list x)) (flat (rest lst)))
             true
              (cons x (flat (rest lst)))))))]
    (cond
      (or (pair? coll) (empty? coll)) (flat coll)
      (array? coll)
       (let [result @[]]
         (each x in (flat (to-list coll)) (push result x))
         result)
      (array? coll)
       (apply array (flat (to-list coll)))
      true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"}))))

(defn take-while [pred coll]
  (letrec
    [tw-list (fn (lst)
       (if (empty? lst)
         ()
         (if (pred (first lst))
           (cons (first lst) (tw-list (rest lst)))
           ())))]
    (cond
      (or (pair? coll) (empty? coll)) (tw-list coll)
      (array? coll)
       (let [result @[]]
         (letrec [loop (fn (i)
                          (when (< i (length coll))
                            (let [x (get coll i)]
                              (when (pred x)
                                (push result x)
                                (loop (+ i 1))))))]
           (loop 0))
         result)
      (array? coll)
       (let [lst (tw-list (letrec [loop (fn (i acc)
                                            (if (>= i (length coll))
                                              (reverse acc)
                                              (loop (+ i 1) (cons (get coll i) acc))))]
                             (loop 0 ())))]
         (apply array lst))
      true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"}))))

(defn drop-while [pred coll]
  (letrec
    [dw-list (fn (lst)
       (if (empty? lst)
         ()
         (if (pred (first lst))
           (dw-list (rest lst))
           lst)))]
    (cond
      (or (pair? coll) (empty? coll)) (dw-list coll)
      (array? coll)
       (letrec [find-start (fn (i)
                              (if (>= i (length coll))
                                (length coll)
                                (if (pred (get coll i))
                                  (find-start (+ i 1))
                                  i)))]
         (let [start (find-start 0)
               result @[]]
           (letrec [loop (fn (i)
                            (when (< i (length coll))
                              (push result (get coll i))
                              (loop (+ i 1))))]
             (loop start))
           result))
      (array? coll)
       (let [lst (dw-list (letrec [loop (fn (i acc)
                                            (if (>= i (length coll))
                                              (reverse acc)
                                              (loop (+ i 1) (cons (get coll i) acc))))]
                             (loop 0 ())))]
         (apply array lst))
      true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"}))))

(defn distinct [coll]
  (let [seen @{}]
    (letrec
      [dist-list (fn (lst)
         (if (empty? lst)
           ()
           (if (has? seen (first lst))
             (dist-list (rest lst))
             (begin (put seen (first lst) true)
                    (cons (first lst) (dist-list (rest lst)))))))]
      (cond
        (or (pair? coll) (empty? coll)) (dist-list coll)
        (array? coll)
         (let [result @[]]
           (each x in coll
             (unless (has? seen x)
               (put seen x true)
               (push result x)))
           result)
        (array? coll)
         (let [lst (dist-list (letrec [loop (fn (i acc)
                                               (if (>= i (length coll))
                                                 (reverse acc)
                                                 (loop (+ i 1) (cons (get coll i) acc))))]
                                (loop 0 ())))]
           (apply array lst))
        true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))))

(defn frequencies [coll]
  (let [counts @{}]
    (each x in coll
      (put counts x (+ 1 (if (has? counts x) (get counts x) 0))))
    (freeze counts)))

(defn mapcat [f coll]
  (cond
    (or (pair? coll) (empty? coll))
     (fold (fn (acc x) (append acc (f x))) () coll)
    (array? coll)
     (let [result @[]]
       (each x in coll
         (each y in (f x) (push result y)))
       result)
    (array? coll)
     (apply array (fold (fn (acc x) (append acc (f x))) ()
                        (letrec [loop (fn (i acc)
                                         (if (>= i (length coll))
                                           (reverse acc)
                                           (loop (+ i 1) (cons (get coll i) acc))))]
                          (loop 0 ()))))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn group-by [f coll]
  (let [groups @{}]
    (each x in coll
      (let [k (f x)]
        (if (has? groups k)
          (push (get groups k) x)
          (put groups k @[x]))))
    groups))

(defn map-indexed [f coll]
  (cond
    (or (pair? coll) (empty? coll))
     (letrec [go (fn (i l)
                    (if (empty? l)
                      ()
                      (cons (f i (first l)) (go (+ i 1) (rest l)))))]
       (go 0 coll))
    (array? coll)
     (let [result @[]]
       (letrec [loop (fn (i)
                        (when (< i (length coll))
                          (push result (f i (get coll i)))
                          (loop (+ i 1))))]
         (loop 0))
       result)
    (array? coll)
     (apply array
       (letrec [go (fn (i)
                      (if (>= i (length coll))
                        ()
                        (cons (f i (get coll i)) (go (+ i 1)))))]
         (go 0)))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn partition [n coll]
  (cond
    (or (pair? coll) (empty? coll))
     (if (or (<= n 0) (empty? coll))
       ()
       (cons (take n coll)
             (partition n (drop n coll))))
    (array? coll)
     (let [result @[]]
       (letrec [loop (fn (i)
                        (when (< i (length coll))
                          (let [chunk @[]]
                            (letrec [inner (fn (j)
                                             (when (and (< j (+ i n)) (< j (length coll)))
                                               (push chunk (get coll j))
                                               (inner (+ j 1))))]
                              (inner i))
                            (push result chunk)
                            (loop (+ i n)))))]
         (loop 0))
       result)
    (array? coll)
     (letrec [to-list (fn (c)
                          (letrec [loop (fn (i acc)
                                          (if (>= i (length c))
                                            (reverse acc)
                                            (loop (+ i 1) (cons (get c i) acc))))]
                            (loop 0 ())))
              part (fn (lst)
                      (if (or (<= n 0) (empty? lst))
                        ()
                        (cons (apply array (take n lst))
                              (part (drop n lst)))))]
       (apply array (part (to-list coll))))
    true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))

(defn interpose [sep coll]
  (letrec
    [ip-list (fn (lst)
       (if (or (empty? lst) (empty? (rest lst)))
         lst
         (cons (first lst)
               (cons sep (ip-list (rest lst))))))]
    (cond
      (or (pair? coll) (empty? coll)) (ip-list coll)
      (array? coll)
       (if (< (length coll) 2)
         coll
         (let [result @[(get coll 0)]]
           (letrec [loop (fn (i)
                            (when (< i (length coll))
                              (push result sep)
                              (push result (get coll i))
                              (loop (+ i 1))))]
             (loop 1))
           result))
      (array? coll)
       (let [lst (ip-list (letrec [loop (fn (i acc)
                                           (if (>= i (length coll))
                                             (reverse acc)
                                             (loop (+ i 1) (cons (get coll i) acc))))]
                             (loop 0 ())))]
         (apply array lst))
      true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"}))))

(defn min-key [f & args]
  (fold (fn (best x) (if (< (f x) (f best)) x best))
        (first args)
        (rest args)))

(defn max-key [f & args]
  (fold (fn (best x) (if (> (f x) (f best)) x best))
        (first args)
        (rest args)))

(defn memoize [f]
  (let [cache @{}]
    (fn (& args)
      (let [key (if (= (length args) 1) (first args) (string args))]
        (if (has? cache key)
          (get cache key)
          (let [result (f ;args)]
            (put cache key result)
            result))))))

(defn sort-by [keyfn coll]
  (letrec
    [to-list (fn (c)
       (cond
         (or (pair? c) (empty? c)) c
         (or (array? c) (array? c))
          (letrec [loop (fn (i acc)
                           (if (>= i (length c))
                             (reverse acc)
                             (loop (+ i 1) (cons (get c i) acc))))]
            (loop 0 ()))
         true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))
     from-list (fn (lst orig)
       (cond
         (or (pair? orig) (empty? orig)) lst
         (array? orig)
          (let [arr @[]]
            (each x in lst (push arr x))
            arr)
         (array? orig) (apply array lst)))
     merge (fn (a b)
       (cond
         (empty? a) b
         (empty? b) a
         (<= (first (first a)) (first (first b)))
          (cons (first a) (merge (rest a) b))
         true
          (cons (first b) (merge a (rest b)))))
     halve (fn (lst)
       (let [mid (/ (length lst) 2)]
         [(take mid lst) (drop mid lst)]))
     msort (fn (lst)
       (if (or (empty? lst) (empty? (rest lst)))
         lst
         (let [[left right] (halve lst)]
           (merge (msort left) (msort right)))))]
    (let* [as-list (to-list coll)
           decorated (map (fn (x) (list (keyfn x) x)) as-list)
           sorted (msort decorated)
           result (map (fn (pair) (first (rest pair))) sorted)]
      (from-list result coll))))

(defn sort-with [cmp coll]
  "Sort coll using comparator (cmp a b) which returns negative, zero, or positive. Stable merge sort. Type-preserving. Alias: sort-by-cmp."
  (letrec
    [to-list (fn (c)
       (cond
         (or (pair? c) (empty? c)) c
         (array? c)
          (letrec [loop (fn (i acc)
                           (if (>= i (length c))
                             (reverse acc)
                             (loop (+ i 1) (cons (get c i) acc))))]
            (loop 0 ()))
         true (error {:error :type-error :reason :not-a-sequence :message "not a sequence"})))
     from-list (fn (lst orig)
       (cond
         (or (pair? orig) (empty? orig)) lst
         (mutable? orig)
          (let [arr @[]]
            (each x in lst (push arr x))
            arr)
         (array? orig) (apply array lst)))
     merge-lists (fn (a b)
       (cond
         (empty? a) b
         (empty? b) a
         (<= (cmp (first a) (first b)) 0)
          (cons (first a) (merge-lists (rest a) b))
         true
          (cons (first b) (merge-lists a (rest b)))))
     halve (fn (lst)
       (let [mid (/ (length lst) 2)]
         [(take mid lst) (drop mid lst)]))
     msort (fn (lst)
       (if (or (empty? lst) (empty? (rest lst)))
         lst
         (let [[left right] (halve lst)]
           (merge-lists (msort left) (msort right)))))]
    (from-list (msort (to-list coll)) coll)))

(def sort-by-cmp sort-with)

## ── Time utilities ──────────────────────────────────────────────────

(defn time/stopwatch []
  (coro/new (fn ()
    (let [start (clock/monotonic)]
      (while true
        (yield (- (clock/monotonic) start)))))))

(defn time/elapsed [thunk]
  (let [start (clock/monotonic)]
    (let [result (thunk)]
      (list result (- (clock/monotonic) start)))))

## ── VM query wrappers ───────────────────────────────────────────────

(defn call-count [f] (vm/query "call-count" f))
(defn global? [sym] (vm/query "global?" sym))
(defn fiber/self [] (vm/query "fiber/self" nil))

(defn fiber/new?    [f] "True if fiber has not yet been resumed."    (= (fiber/status f) :new))
(defn fiber/alive?  [f] "True if fiber is currently executing."      (= (fiber/status f) :alive))
(defn fiber/paused? [f] "True if fiber is paused (waiting to resume)." (= (fiber/status f) :paused))
(defn fiber/dead?   [f] "True if fiber completed normally."          (= (fiber/status f) :dead))
(defn fiber/error?  [f] "True if fiber terminated with an error."    (= (fiber/status f) :error))

## ── Arena introspection ─────────────────────────────────────────────



## ── Control flow graph rendering ────────────────────────────────────

(defn fn/cfg (target & opts)
  "Render a closure or fiber's control flow graph as text.
   Optional format keyword: :mermaid (default) or :dot.
   (fn/cfg my-fn)          => Mermaid flowchart string
   (fn/cfg my-fn :dot)     => DOT digraph string
   (fn/cfg my-fn :mermaid) => Mermaid flowchart string"
  (let* [fmt (if (empty? opts)
                :mermaid
                (if (> (length opts) 1)
                  (error {:error :arity-error :reason :too-many-args :maximum 1 :message "expected at most 1 format keyword"})
                  (first opts)))
         cfg (fn/flow target)]
    (when (nil? cfg)
      (error {:error :type-error :reason :no-lir :message "target has no LIR"}))
    (cond
      (= fmt :mermaid) (fn/cfg-mermaid cfg)
      (= fmt :dot)     (fn/cfg-dot cfg)
      true (error {:error :type-error :reason :unknown-format :format fmt :expected |:mermaid :dot| :message (string "unknown format: " fmt)}))))

(defn fn/cfg-label (cfg)
  "Build the label string from a CFG struct's metadata."
  (let* [name (get cfg :name)
         doc (get cfg :doc)]
    (if (nil? name)
      (if (nil? doc) "anonymous" doc)
      name)))

(defn fn/cfg-dot (cfg)
  "Render a CFG struct as a DOT digraph string with compact instructions."
  (letrec [dot-escape (fn (s)
             (-> s
               (string/replace "\"" "\\\"")
               (string/replace "{" "\\{")
               (string/replace "}" "\\}")
               (string/replace "|" "\\|")
               (string/replace "<" "\\<")
               (string/replace ">" "\\>")))]
    (let [@result (-> "digraph {\n  label=\""
                    (append (dot-escape (string/replace (fn/cfg-label cfg) "\n" " ")))
                    (append " arity:")
                    (append (get cfg :arity))
                    (append " regs:")
                    (append (string (get cfg :regs)))
                    (append " locals:")
                    (append (string (get cfg :locals)))
                    (append "\";\n  node [shape=record fontname=\"monospace\" fontsize=10];\n"))]
      (each block (get cfg :blocks)
        (let* [lbl (string (get block :label))
               display (get block :annotated)
               term-display (get block :term-display)
               term-kind (get block :term-kind)
               edges (get block :edges)
               color (cond
                        (= term-kind :return) "#4444cc"
                        (= term-kind :branch) "#cc8800"
                        (= term-kind :yield)  "#008844"
                        true                  "#444444")]
          (assign result (-> result
                        (append "  block")
                        (append lbl)
                        (append " [color=\"")
                        (append color)
                        (append "\" label=\"{block")
                        (append lbl)))
          (assign result (append result "|"))
          (each instr display
            (assign result (-> result
                          (append (dot-escape instr))
                          (append "\\l"))))
          (assign result (-> result
                        (append "|")
                        (append (dot-escape term-display))
                        (append "}\"];\n")))
          (each edge edges
            (assign result (-> result
                          (append "  block")
                          (append lbl)
                          (append " -> block")
                          (append (string edge))
                          (append ";\n"))))))
      (append result "}\n"))))

(defn fn/cfg-mermaid (cfg)
  "Render a CFG struct as a Mermaid flowchart with visual distinctions."
  (letrec [mmd-escape (fn (s)
             (-> s
               (string/replace "&" "&amp;")
               (string/replace "\"" "&quot;")))]
    (let [@result (-> "flowchart TD\n"
                    (append "  %% ")
                    (append (string/replace (fn/cfg-label cfg) "\n" " "))
                    (append " arity:")
                    (append (get cfg :arity))
                    (append " regs:")
                    (append (string (get cfg :regs)))
                    (append " locals:")
                    (append (string (get cfg :locals)))
                    (append "\n")
                    (append "  classDef entry fill:#d4edda,stroke:#28a745,stroke-width:2px\n")
                    (append "  classDef ret fill:#cce5ff,stroke:#004085,stroke-width:2px\n")
                    (append "  classDef branch fill:#fff3cd,stroke:#856404,stroke-width:2px\n")
                    (append "  classDef yield_block fill:#d1ecf1,stroke:#0c5460,stroke-width:2px\n")
                    (append "  classDef normal fill:#f8f9fa,stroke:#6c757d\n"))]
      (each block (get cfg :blocks)
        (let* [lbl (string (get block :label))
               display (get block :display)
               term-display (get block :term-display)
               term-kind (get block :term-kind)
               edges (get block :edges)
               # Choose node shape based on terminator kind
               # All labels are quoted to avoid parser issues with special chars
               open-delim (cond
                             (= term-kind :branch) "{\""
                             (= term-kind :return) "([\""
                             (= term-kind :yield)  "{{\""
                             true                  "[\"")
               close-delim (cond
                              (= term-kind :branch) "\"}"
                              (= term-kind :return) "\"])"
                              (= term-kind :yield)  "\"}}"
                              true                  "\"]")
               # Build node content with compact instructions
               @content (-> (append "block" lbl)
                          (append "<br/>"))]
           # Add each instruction
          (each instr display
            (assign content (-> content
                           (append "<br/>")
                           (append (mmd-escape instr)))))
          # Add terminator separator and terminator
          (assign content (-> content
                         (append "<br/>---<br/>")
                         (append (mmd-escape term-display))))
          # Emit node with shape
          (assign result (-> result
                        (append "  block")
                        (append lbl)
                        (append open-delim)
                        (append content)
                        (append close-delim)
                        (append "\n")))
          # Apply style class
          (let [cls (cond
                       (= lbl (string (get cfg :entry))) "entry"
                       (= term-kind :return)  "ret"
                       (= term-kind :branch)  "branch"
                       (= term-kind :yield)   "yield_block"
                       true                   "normal")]
            (assign result (-> result
                          (append "  class block")
                          (append lbl)
                          (append " ")
                          (append cls)
                          (append "\n"))))
           # Emit edges
           (each edge edges
             (assign result (-> result
                           (append "  block")
                           (append lbl)
                           (append " --> block")
                           (append (string edge))
                           (append "\n"))))))
       result)))

## ── Struct operations ───────────────────────────────────────────────

(defn merge [a b]
  "Merge struct b into struct a. Both must be structs of the same mutability.
   Keys in b override keys in a. Returns the same mutability as the inputs.
   Signals :type-error if either argument is not a struct or mutabilities differ."
  (if (not (struct? a))
    (error {:error :type-error :reason :not-a-struct :position :first :message "first argument must be a struct"})
    (if (not (struct? b))
      (error {:error :type-error :reason :not-a-struct :position :second :message "second argument must be a struct"})
      (if (not (= (mutable? a) (mutable? b)))
        (error {:error :type-error :reason :mutability-mismatch :message "both arguments must be the same mutability"})
        (let [result (@struct)]
          (each k in (keys a) (put result k (get a k)))
          (each k in (keys b) (put result k (get b k)))
          (if (mutable? a) result (freeze result)))))))

## ── Stream combinators ─────────────────────────────────────────────
##
## Streams are coroutines. A read stream yields values when resumed.
## Sink combinators consume a stream to completion and return a result.
## Transform combinators return a new coroutine wrapping a source.
## Port-to-stream converters return coroutines backed by an open port.
##
## All port-backed streams must be consumed inside a scheduler context
## (ev/spawn or ev/with-scheduler) because port I/O emits SIG_IO.

(defn stream/for-each [f source]
  "Apply f to each value yielded by source. Returns nil.
   Signal is polymorphic in f: if f yields, stream/for-each yields."
  (coro/resume source)
  (while (not (coro/done? source))
    (f (coro/value source))
    (coro/resume source))
  nil)

(defn stream/fold [f init source]
  "Reduce values from source with f, starting from init.
   Returns the final accumulator value.
   Signal is polymorphic in f: if f yields, stream/fold yields."
  (def @acc init)
  (coro/resume source)
  (while (not (coro/done? source))
    (assign acc (f acc (coro/value source)))
    (coro/resume source))
  acc)

(defn stream/collect [source]
  "Collect all values yielded by source into a list.
   Builds in reverse using cons then reverses — O(n).
   Signal: errors only (no user callback)."
  (def @acc ())
  (coro/resume source)
  (while (not (coro/done? source))
    (assign acc (cons (coro/value source) acc))
    (coro/resume source))
  (reverse acc))

(defn stream/into-array [source]
  "Collect all values yielded by source into a mutable array.
   Signal: errors only (no user callback)."
  (let [result @[]]
    (coro/resume source)
    (while (not (coro/done? source))
      (push result (coro/value source))
      (coro/resume source))
    result))

(defn stream/map [f source]
  "Return a coroutine that yields (f value) for each value from source.
   Signal: Silent (may error). f is not called at construction time."
  (coro/new (fn []
    (forever
      (coro/resume source)
      (if (coro/done? source)
        (break)
        (yield (f (coro/value source))))))))

(defn stream/filter [pred source]
  "Return a coroutine that yields values from source where (pred value) is truthy.
   Signal: Silent (may error). pred is not called at construction time."
  (coro/new (fn []
    (forever
      (coro/resume source)
      (when (coro/done? source) (break))
      (when (pred (coro/value source))
        (yield (coro/value source)))))))

(defn stream/take [n source]
  "Return a coroutine that yields at most n values from source.
   Signal: Silent (may error)."
  (coro/new (fn []
    (def @remaining n)
    (forever
      (when (<= remaining 0) (break))
      (coro/resume source)
      (when (coro/done? source) (break))
      (yield (coro/value source))
      (assign remaining (- remaining 1))))))

(defn stream/drop [n source]
  "Return a coroutine that skips n values from source, then yields the rest.
   Signal: Silent (may error)."
  (coro/new (fn []
    (def @skipped 0)
    # Skip n values
    (while (< skipped n)
      (coro/resume source)
      (when (coro/done? source) (break))
      (assign skipped (+ skipped 1)))
    # Yield the rest
    (when (not (coro/done? source))
      (forever
        (coro/resume source)
        (when (coro/done? source) (break))
        (yield (coro/value source)))))))

(defn stream/concat [& sources]
  "Return a coroutine that yields all values from each source in order.
   Dead (pre-exhausted) sources are skipped gracefully.
   Signal: Silent (may error)."
  (coro/new (fn []
    (each src in sources
      # Guard against resuming an already-dead coroutine — coro/resume
      # on a dead coroutine is an error, not a no-op.
      (when (not (coro/done? src))
        (coro/resume src)
        (while (not (coro/done? src))
          (yield (coro/value src))
          (coro/resume src)))))))

(defn stream/zip [& sources]
  "Return a coroutine that yields immutable arrays of values, one from each source.
   Stops when any source is exhausted (shortest-wins semantics).
   Signal: Silent (may error)."
  (coro/new (fn []
    (forever
      (def @done false)
      (let [vals (map (fn [s]
                        (coro/resume s)
                        (when (coro/done? s) (assign done true))
                        (coro/value s))
                      sources)]
        (when done (break))
        (yield (apply array vals)))))))

(defn stream/pipe [source & transforms]
  "Thread source through each transform function in order.
   Each transform is (fn [stream] -> stream).
   Example: (stream/pipe src (partial stream/map f) (partial stream/take 10))
   Signal: polymorphic in transforms."
  (fold (fn [s t] (t s)) source transforms))

(defn port/lines [port]
  "Yields lines from port one at a time. Closes port on exhaustion.
   Must be called inside a scheduler context (ev/spawn)."
  (coro/new (fn []
    (forever
      (let [line (port/read-line port)]
        (if (nil? line)
          (begin (port/close port) (break))
          (yield line)))))))

(defn port/chunks [port size]
  "Yields byte chunks of `size` from port. Final chunk may be smaller.
   Must be called inside a scheduler context."
  (coro/new (fn []
    (forever
      (let [chunk (port/read port size)]
        (if (nil? chunk)
          (begin (port/close port) (break))
          (yield chunk)))))))

(defn port/writer [port]
  "Returns a write-stream coroutine. Resume with a string to write it.
   Resume with nil to close the port. Must be called inside a scheduler context."
  (coro/new (fn []
    (forever
      (let [val (yield nil)]
        (if (nil? val)
          (begin (port/close port) (break))
          (port/write port val)))))))

## ── Standard port parameters ────────────────────────────────────────

(def *stdin*  (parameter (port/stdin)))
(def *stdout* (parameter (port/stdout)))
(def *stderr* (parameter (port/stderr)))

## ── Scheduler ───────────────────────────────────────────────────────

(def *spawn* (make-parameter nil))
(def *scheduler* (make-parameter nil))
(def *io-backend* (make-parameter nil))

## ── Output ──────────────────────────────────────────────────────────

(defn print [& args]
  "Write values to *stdout*, no newline. Respects *stdout* rebinding."
  (let [stdout (*stdout*)]
    (port/write stdout (apply string args))
    (port/flush stdout)))

(defn println [& args]
  "Write values to *stdout* with trailing newline. Respects *stdout* rebinding."
  (let [stdout (*stdout*)]
    (port/write stdout (string (apply string args) "\n"))
    (port/flush stdout)))

(defn eprint [& args]
  "Write values to *stderr*, no newline. Respects *stderr* rebinding."
  (let [stderr (*stderr*)]
    (port/write stderr (apply string args))
    (port/flush stderr)))

(defn eprintln [& args]
  "Write values to *stderr* with trailing newline. Respects *stderr* rebinding."
  (let [stderr (*stderr*)]
    (port/write stderr (string (apply string args) "\n"))
    (port/flush stderr)))

## ── Spawn ───────────────────────────────────────────────────────────

(defn ev/spawn [closure]
    "Spawn a closure in a new fiber managed by the current scheduler."
    (let [fiber (fiber/new closure |:error :io :exec :wait|)]
      ((*spawn*) fiber)))

## ── Async scheduler ─────────────────────────────────────────────────

(defn make-async-scheduler ()
  "Create an async scheduler. Returns {:spawn fn :pump fn :shutdown fn}.
   :spawn — (fn [fiber]) registers fiber for async execution.
   :pump — (fn []) pumps event loop until all fibers complete.
   :shutdown — (fn [timeout-ms]) signal shutdown; pump-fn executes it."
  (let [backend       (io/backend :async)
        runnable      @[]
        pending       @{}        # id → fiber (I/O submissions)
        fiber-io      @{}        # fiber → id (reverse lookup for io/cancel)
        waiters       @{}        # target-fiber → @[waiting-fibers...]
        select-sets   @{}        # waiting-fiber → @{:candidates [...] :woken @[false]}
        completed     @{}        # fiber → :ok | :error (already-completed fibers)
        joined        @|  |      # set of fibers whose result was observed
        shutdown-req  @[nil]     # nil = running, integer = shutdown requested with timeout
        park-queues   @{}]       # key → @[parked-fibers...] (futex park/notify)

    (defn cleanup-select [waiter entry]
      "Delete a select-set entry after resolution."
      (del select-sets waiter))

    (defn wake-select-waiters [fiber]
      "Wake any select-set waiter that includes fiber as a candidate."
      (each [waiter entry] in (pairs select-sets)
        (when (not (nil? (find (fn [candidate] (= candidate fiber)) (get entry :candidates))))
          (let [woken (get entry :woken)]
            (when (not (get woken 0))
              (put woken 0 true)
              (cleanup-select waiter entry)
              (fiber/resume waiter fiber)
              (handle-fiber-after-resume waiter))))))

    (defn complete-fiber [fiber status]
      "Handle fiber completion: wake join and select waiters."
      # Record completion
      (put completed fiber status)
      # Clean up fiber-io mapping
      (let [id (get fiber-io fiber)]
        (when (not (nil? id))
          (del fiber-io fiber)))
      # Wake join waiters with [ok? value] pair
      (let [ws (get waiters fiber)]
        (when (not (nil? ws))
          (del waiters fiber)
          (let [pair [(= status :ok) (fiber/value fiber)]]
            (each w in ws
              (fiber/resume w pair)
              (handle-fiber-after-resume w))))
        # Wake select waiters
        (wake-select-waiters fiber)))

    (defn get-completion [fiber]
      "Return fiber's completion status (:ok or :error), or nil if the fiber
       has not yet terminated. Lazily records completion from the fiber's raw
       status — if the fiber transitioned to :dead/:error via a path the
       scheduler observed only indirectly (e.g. while handling another fiber),
       record the completion atomically the first time we inspect it. This
       keeps `completed` in sync with reality and lets handle-abort /
       handle-join / handle-select make consistent decisions."
      (let [recorded (get completed fiber)]
        (if (not (nil? recorded))
          recorded
          (case (fiber/status fiber)
            :dead  (begin (complete-fiber fiber :ok)    :ok)
            :error (begin (complete-fiber fiber :error) :error)
            nil))))

    (defn handle-join [caller target]
      "Handle a :join wait request. Resumes caller with [ok? value]."
      (add joined target)
      (let [comp (get-completion target)]
        (if (not (nil? comp))
          # Already completed — resume immediately
          (begin (fiber/resume caller [(= comp :ok) (fiber/value target)])
                 (handle-fiber-after-resume caller))
          # Still running — park caller on target's join waiter list
          (let [ws (or (get waiters target)
                        (let [w @[]] (put waiters target w) w))]
            (push ws caller)))))

    (defn handle-select [caller candidates]
      "Handle a :select wait request."
      # Check if any candidate already completed (records completion lazily
      # via get-completion so the scheduler stays consistent).
      (let [done (find (fn [f] (not (nil? (get-completion f))))
                        candidates)]
        (if done
          # Immediate: resume with the completed fiber
          (begin (fiber/resume caller done)
                 (handle-fiber-after-resume caller))
          # Park with a select set — wake-select-waiters scans select-sets directly,
          # so we don't add to the waiters map (that's for join waiters only).
          (let [entry @{:candidates candidates :woken @[false]}]
            (put select-sets caller entry)))))

    (defn handle-abort [caller target]
      "Handle an :abort wait request."
      (add joined target)
      # get-completion records the completion if target has already
      # transitioned to :dead/:error but the scheduler hadn't noticed yet.
      # Without this, the guard below would pass and fiber/abort would be
      # called on an already-terminal target (state-error without Option A,
      # silent no-op with it — but either way, the abort logic below is
      # wrong on a dead fiber).
      (when (nil? (get-completion target))
        # Cancel pending I/O if any
        (let [id (get fiber-io target)]
          (when (not (nil? id))
            (io/cancel backend id)
            (del pending id)
            (del fiber-io target)))
        # Graceful abort (runs defer/protect)
        (fiber/abort target {:error :aborted})
        # Route the aborted fiber through completion
        (handle-fiber-after-resume target))
      # Resume caller with nil
      (fiber/resume caller nil)
      (handle-fiber-after-resume caller))

    (defn handle-park [caller request]
      "Handle a :park wait request (futex wait).
       If cell value == expected, park caller. Otherwise resume immediately."
      (let* [key      (request :key)
             val-cell (request :val)
             expected (request :expected)]
        (if (= (get val-cell 0) expected)
          # Value matches — park the fiber (stays suspended)
          (let [q (or (park-queues key)
                       (let [q @[]] (put park-queues key q) q))]
            (push q caller))
          # Value changed — spurious wakeup avoidance, resume immediately
          (begin (fiber/resume caller :ok)
                 (handle-fiber-after-resume caller)))))

    (defn handle-notify [caller request]
      "Handle a :notify wait request (futex wake).
       Wake min(count, queue-length) parked fibers, resume caller with woken count."
      (let* [key   (request :key)
             count (request :count)
             q     (or (park-queues key) @[])
             n     (min count (length q))
             @woken 0]
        (def @i 0)
        (while (< i n)
          (let [fiber (q 0)]
            (remove q 0)
            (fiber/resume fiber true)
            (push runnable fiber))
          (assign i (inc i)))
        (assign woken i)
        # Remove empty queue
        (when (= (length q) 0)
          (del park-queues key))
        # Resume caller immediately with woken count
        (fiber/resume caller woken)
        (handle-fiber-after-resume caller)))

    (defn handle-wait [caller request]
      "Dispatch a :wait signal based on :op."
      (case (request :op)
        :join   (handle-join caller (request :fiber))
        :select (handle-select caller (request :fibers))
        :abort  (handle-abort caller (request :fiber))
        :park   (handle-park caller request)
        :notify (handle-notify caller request)
        (error {:error :protocol-error
                :reason :unknown-op :op (request :op) :message (string "unknown op: " (request :op))})))

    (defn handle-fiber-after-resume [fiber]
      "Route a fiber to the right place after resume."
      (case (fiber/status fiber)
        :dead   (complete-fiber fiber :ok)
        :error  (complete-fiber fiber :error)
        :paused (let [bits (fiber/bits fiber)]
                  (cond
                    (not (= 0 (bit/and bits 1)))       # SIG_ERROR
                     (complete-fiber fiber :error)
                    (not (= 0 (bit/and bits 512)))     # SIG_IO
                     (let [[ok? result] (protect (io/submit backend (fiber/value fiber)))]
                       (if ok?
                         (begin
                           (put pending result fiber)
                           (put fiber-io fiber result))
                         (begin
                           (fiber/abort fiber result)
                           (handle-fiber-after-resume fiber))))
                    (not (= 0 (bit/and bits 16384)))   # SIG_WAIT (bit 14)
                     (handle-wait fiber (fiber/value fiber))
                    true
                     (push runnable fiber)))))

    (defn drain-runnable []
      "Run all runnable fibers. Guard against externally-killed fibers."
      (while (> (length runnable) 0)
        (let [fiber (pop runnable)]
          (let [status (fiber/status fiber)]
            (cond
              (= status :dead)  (complete-fiber fiber :ok)
              (= status :error) (complete-fiber fiber :error)
              true (begin (fiber/resume fiber)
                           (handle-fiber-after-resume fiber)))))))

    (defn process-completions [timeout-ms]
      "Wait for I/O completions and route fibers."
      (let [completions (io/wait backend timeout-ms)]
        (each c in completions
          (let* [id    (get c :id)
                 fiber (get pending id)]
            (when (not (nil? fiber))
              (del pending id)
              (del fiber-io fiber)
              (if (nil? (get c :error))
                (begin
                  (fiber/resume fiber (get c :value))
                  (handle-fiber-after-resume fiber))
                # I/O error: inject error into the fiber so it propagates
                # through protect/defer correctly.
                (begin
                  (fiber/abort fiber (get c :error))
                  (handle-fiber-after-resume fiber))))))))

    (defn do-shutdown [timeout-ms]
      "Abort all pending fibers, pump for timeout-ms, cancel stragglers."
      # Phase 1: abort all pending fibers (inject error, let defer run).
      (each [id fiber] in (pairs pending)
        (del pending id)
        (del fiber-io fiber)
        (io/cancel backend id)
        (let [[ok? _] (protect (fiber/abort fiber {:error :shutdown}))]
          (when ok? (handle-fiber-after-resume fiber))))
      # Phase 2: drain cancel CQEs and let aborted fibers unwind
      (when (> timeout-ms 0)
        (let [deadline (+ (clock/monotonic) (/ timeout-ms 1000.0))]
          (while (and (> (+ (length runnable) (length pending)) 0)
                      (< (clock/monotonic) deadline))
            (drain-runnable)
            (when (> (length pending) 0)
              (let [completions (io/wait backend 10)]
                (each c in completions
                  (let* [id    (get c :id)
                         fiber (get pending id)]
                    (when (not (nil? fiber))
                      (del pending id)
                      (del fiber-io fiber)
                      (when (nil? (get c :error))
                        (fiber/resume fiber (get c :value))
                        (handle-fiber-after-resume fiber))))))))))
      # Phase 3: cancel any stragglers and their pending I/O
      (each [id fiber] in (pairs pending)
        (del pending id)
        (del fiber-io fiber)
        (io/cancel backend id)
        (protect (fiber/cancel fiber {:error :shutdown})))
      (while (> (length runnable) 0)
        (let [fiber (pop runnable)]
          (protect (fiber/cancel fiber {:error :shutdown})))))

    (defn step [timeout-ms]
      "Execute one tick of the event loop. Returns :done or :pending."
      (block :tick
        (drain-runnable)
        (when (and (= (length pending) 0)
                   (= (length waiters) 0)
                   (= (length select-sets) 0)
                   (= (length park-queues) 0))
          (break :tick :done))
        (let [timeout (get shutdown-req 0)]
          (unless (nil? timeout)
            (do-shutdown timeout)
            (break :tick :done)))
        (process-completions timeout-ms)
        :pending))

    {:spawn
      # scheduler-fn: register fiber
      (fn (fiber)
        (push runnable fiber)
        fiber)
     :step step
     :pump
      # pump-fn: event loop
      (fn ()
        (block :loop
          (forever
            (when (= (step (- 0 1)) :done)
              (break :loop nil))))
        # Crash on unjoined errored fibers — never swallow errors silently
        (each [fiber status] in (pairs completed)
          (when (and (= status :error) (not (contains? joined fiber)))
            (error (fiber/value fiber)))))
     :shutdown
      # shutdown-fn: signal shutdown
      (fn (timeout-ms)
        (put shutdown-req 0 timeout-ms))
     :mark-joined
      # mark a fiber as observed (suppress unjoined-error crash)
      (fn (fiber) (add joined fiber))
     :backend backend}))

(def *shutdown* (make-parameter nil))

(defn ev/shutdown [& args]
  "Shut down the current event loop. Optional timeout-ms (default 0) gives
   fibers time to clean up before being hard-killed."
  (let [timeout-ms (or (get args 0) 0)
        shutdown-fn (*shutdown*)]
    (when (nil? shutdown-fn)
      (error {:error :state-error :reason :no-event-loop :message "not inside an event loop"}))
    (shutdown-fn timeout-ms)))

(defn ev/step [& args]
  "Step the current event loop once. timeout-ms defaults to 0 (non-blocking).
   Returns :done when all fibers have completed, :pending otherwise."
  (let [timeout (or (get args 0) 0)]
    ((get (*scheduler*) :step) timeout)))

(defn ev/with-scheduler [sched & thunks]
  "Run thunks under the given scheduler.
   sched is a scheduler struct from make-async-scheduler (has :spawn, :pump, :shutdown).
   Parameterizes *scheduler*, *spawn*, and *shutdown*; spawns each thunk; pumps until done."
  (parameterize ((*scheduler* sched)
                 (*spawn* (get sched :spawn))
                 (*shutdown* (get sched :shutdown))
                 (*io-backend* (get sched :backend)))
    (each t in thunks
      (ev/spawn t))
    ((get sched :pump))))

(defn ev/run (& thunks)
  "Create an async scheduler, run thunks, return the last thunk's result.
   Used internally by the compiler to wrap top-level code in a scheduler.
   Propagates errors from fibers — unjoined errored fibers crash the process."
  (let [sched (make-async-scheduler)]
    (parameterize ((*scheduler* sched)
                   (*spawn* (get sched :spawn))
                   (*shutdown* (get sched :shutdown))
                   (*io-backend* (get sched :backend)))
      (let [mark (get sched :mark-joined)
            fibers @[]]
        (each t in thunks
          (push fibers (ev/spawn t)))
        ((get sched :pump))
        # Mark all entry-point fibers as joined — they're owned by ev/run,
        # not orphaned.  Propagate the first error we find among them.
        (def @result nil)
        (def @first-error nil)
        (each f in fibers
          (mark f)
          (let [s (fiber/status f)]
            (when (and (nil? first-error)
                       (or (= s :error)
                           (not (= 0 (bit/and (fiber/bits f) 1)))))
              (assign first-error (fiber/value f)))))
        (when (not (nil? first-error))
          (error first-error))
        # Return the last fiber's value
        (when (> (length fibers) 0)
          (assign result (fiber/value (get fibers (- (length fibers) 1)))))
        result))))

## ── Structured concurrency primitives ───────────────────────────────

(defn emit-wait [request]
  "Emit a :wait signal. Guards against use outside async scheduler."
  (when (nil? (*spawn*))
    (error {:error :state-error
            :reason :no-scheduler :op (get request :op) :message (string (get request :op) " requires an async scheduler")}))
  (emit :wait request))

(defn ev/futex-wait [key cell expected]
  "Park the current fiber if (get cell 0) == expected. Returns when woken
   or immediately if the value has already changed (spurious wakeup avoidance).
   key must be a unique hashable value identifying this futex."
  (emit-wait {:op :park :key key :val cell :expected expected}))

(defn ev/futex-wake [key count]
  "Wake up to count fibers parked on key. Returns the number actually woken.
   Caller is NOT suspended — returns immediately."
  (emit-wait {:op :notify :key key :count count}))

(defn ev/join [target]
  "Wait for a fiber or sequence of fibers, returning their results.
   Single fiber: returns value or propagates error.
   Sequence: joins each in order, returns array of results."
  (if (fiber? target)
    (let [[ok? val] (emit-wait {:op :join :fiber target})]
      (if ok? val (error val)))
    # Sequence of fibers
    (let [results @[]]
      (each f in target
        (push results (ev/join f)))
      (freeze results))))

(defn ev/join-protected [target]
  "Like ev/join but never signals an error. Returns [ok? value].
   Sequence: returns [[ok? value] ...]."
  (if (fiber? target)
    (emit-wait {:op :join :fiber target})
    # Sequence of fibers
    (let [results @[]]
      (each f in target
        (push results (ev/join-protected f)))
      (freeze results))))

(defn ev/abort [target]
  "Abort a fiber gracefully via the scheduler. defer/protect blocks run.
   No-op if the fiber is already completed."
  (emit-wait {:op :abort :fiber target}))

(defn ev/as-completed [fibers]
  "Return [next-fn pool] for iterating fibers in completion order.
   next-fn returns the next completed fiber, or nil when done.
   pool is a mutable array; push new fibers to it for backfill."
  (let [pool @[]]
    (each f in fibers (push pool f))
    [(fn []
       (if (empty? pool)
         nil
         (let [done (emit-wait {:op :select :fibers pool})]
           (let [i (find-index (fn [f] (= f done)) pool)]
             (when i (remove pool i)))
           done)))
     pool]))

(defn ev/select [fibers]
  "Wait for the first of N fibers to complete.
   Returns [completed-fiber remaining-fibers]."
  (let [[next ignore] (ev/as-completed fibers)]
    (let [done (next)]
      [done (filter (fn [f] (not (= f done))) fibers)])))

(defn ev/race [fibers]
  "Wait for the first fiber to complete, abort all others, return winner's value."
  (let [[done remaining] (ev/select fibers)]
    (each f in remaining (ev/abort f))
    (ev/join done)))

(defn ev/timeout [seconds thunk]
  "Run thunk with a time limit. Returns result or signals {:error :timeout}."
  (let [work  (ev/spawn thunk)
        timer (ev/spawn (fn [] (ev/sleep seconds)))]
    (let [[done remaining] (ev/select [work timer])]
      (each f in remaining (ev/abort f))
      (if (= done work)
        (ev/join work)
        (error {:error :timeout :reason :deadline-exceeded :message "operation timed out"})))))

(defn ev/scope [body-fn]
  "Structured concurrency nursery. body-fn receives a spawn function.
   All spawned fibers must complete before scope exits.
   If any fiber (body or child) errors, all remaining siblings are aborted
   immediately — the scope does not wait for the body to finish first."
  (let* [scope-fibers @[]
         [next pool] (ev/as-completed @[])
         scope-spawn (fn [closure]
                        (let [f (ev/spawn closure)]
                          (push scope-fibers f)
                          (push pool f)
                          f))]
    (let [body-fiber (ev/spawn (fn [] (body-fn scope-spawn)))]
      (push scope-fibers body-fiber)
      (push pool body-fiber)
      # Monitor all fibers — detect errors from any fiber immediately
      (def @body-val nil)
      (def @first-error nil)
      (block :done
        (forever
          (let [done (next)]
            (when (nil? done) (break :done nil))
            # Use ev/join-protected to get the scheduler's view of completion
            # status (fiber/status stays :paused for caught errors).
            (let [[ok? val] (ev/join-protected done)]
              (when (= done body-fiber)
                (assign body-val val))
              (when (and (not ok?) (nil? first-error))
                (assign first-error val)
                # Abort all remaining fibers — check :paused (running/waiting)
                # and :new (not yet started). handle-abort is a no-op for
                # already-completed fibers.
                (each f in scope-fibers
                  (let [s (fiber/status f)]
                    (when (or (= s :paused) (= s :new))
                      (ev/abort f)))))))))
      (if (not (nil? first-error))
        (error first-error)
        body-val))))

(defn ev/map [f items]
  "Apply f to each item concurrently, return results in input order."
  (ev/join (map (fn [x] (ev/spawn (fn [] (f x)))) items)))

(defn ev/map-limited [f items limit]
  "Like ev/map, but with at most limit fibers in flight at once.
   Results are returned in input order."
  (def @todo (apply list items))
  (def @n 0)
  (let [fiber->idx @{}
        results @{}]
   (let [[next pool] (ev/as-completed @[])]
    # Seed the pool
    (while (and (not (empty? todo)) (< (length pool) limit))
      (let* [item (first todo)
             fiber (ev/spawn (fn [] (f item)))]
        (assign todo (rest todo))
        (put fiber->idx fiber n)
        (push pool fiber)
        (assign n (+ n 1))))
    # Drain + backfill
    (forever
      (let [done (next)]
        (when (nil? done) (break nil))
        (put results (get fiber->idx done) (fiber/value done))
        (when (not (empty? todo))
          (let* [item (first todo)
                 fiber (ev/spawn (fn [] (f item)))]
            (assign todo (rest todo))
            (put fiber->idx fiber n)
            (push pool fiber)
            (assign n (+ n 1))))))
    # Collect in input order
    (map (fn [i] (get results i)) (range 0 n)))))

(defn inc [x]
  "Return x + 1."
  (+ x 1))

(defn dec [x]
  "Return x - 1."
  (- x 1))

(defn any? [pred coll]
  "Return true if any value in the sequence is truthy. Short-circuits."
  (each x in coll
        (when (pred x)
        (break true))))

(defn all? [pred coll]
  "Return true if pred is truthy for every element. Short-circuits on first failure."
  (not (any? (fn [x] (not (pred x))) coll)))

(defn find [pred coll]
  "Return the first value in the sequence where (pred value) is truthy. Short-circuits.
   Returns nil if no such value is found."
  (each x in coll
        (when (pred x)
        (break x))))

## ── Subprocess convenience ────────────────────────────────────────────

(defn subprocess/system [program args & opts]
  "Run a command to completion, capturing stdout and stderr as text.
   Returns {:exit int :stdout string :stderr string}.
   Optional third argument: opts struct with keys:
     :env   — struct of env vars (default: inherit)
     :cwd   — working directory string (default: inherit)
     :stdin — :null (default) | :pipe | :inherit
   Output is decoded as strict UTF-8. If subprocess produces invalid
   UTF-8, the error propagates.

   IMPORTANT: reads pipes BEFORE subprocess/wait to avoid deadlock. If subprocess
   output exceeds the OS pipe buffer (~64KB), the subprocess blocks on write.
   Reading first ensures neither side is blocked."
  (let* [exec-opts (if (empty? opts)
                       {:stdin :null}
                       (merge {:stdin :null} (freeze (first opts))))
         proc         (subprocess/exec program args exec-opts)
         # Drain pipes BEFORE subprocess/wait (deadlock invariant — see docstring).
         # port/read-all returns nil on immediate EOF (empty pipe) — coerce to empty bytes.
         stdout-bytes (if (nil? (get proc :stdout))
                         (bytes)
                         (let [raw (port/read-all (get proc :stdout))]
                           (if (nil? raw) (bytes) raw)))
         stderr-bytes (if (nil? (get proc :stderr))
                         (bytes)
                         (let [raw (port/read-all (get proc :stderr))]
                           (if (nil? raw) (bytes) raw)))
         exit-code    (subprocess/wait proc)]
    (when (not (nil? (get proc :stdout))) (port/close (get proc :stdout)))
    (when (not (nil? (get proc :stderr))) (port/close (get proc :stderr)))
    {:exit   exit-code
     :stdout (string stdout-bytes)
     :stderr (string stderr-bytes)}))

## ── FFI helpers ─────────────────────────────────────────────────────

(defn ffi/pin [b]
  "Copy bytes/string to a malloc'd pointer. Caller must ffi/free.
   (ffi/pin (bytes 1 2 3)) => <pointer ...>"
  (let* [b (if (string? b) (bytes b) b)
         len (length b)
         ptr (ffi/malloc (max len 1))]
    (when (> len 0)
      (ffi/write ptr (ffi/array :u8 len) b))
    ptr))

## ── Collection helpers ─────────────────────────────────────────────

(defn from-pairs [pairs]
  "Build a struct from a sequence of key-value pairs (arrays or lists).
   (from-pairs [[:a 1] [:b 2]]) => {:a 1 :b 2}
   (from-pairs (pairs {:x 1})) => {:x 1}"
  (let [result @{}]
    (each pair in pairs
      (put result (first pair) (first (rest pair))))
    (freeze result)))

(defn get-in [coll keys]
  "Get a value at a nested key path.
   (get-in {:a {:b 1}} [:a :b]) => 1"
  (fold get coll keys))

(defn put-in [coll keys val]
  "Put a value at a nested key path, returning a new collection.
   (put-in {:a {:b 1}} [:a :b] 2) => {:a {:b 2}}"
  (if (= (length keys) 1)
    (put coll (first keys) val)
    (put coll (first keys)
         (put-in (get coll (first keys)) (rest keys) val))))

(defn update-in [coll keys f]
  "Apply f to the value at a nested key path.
   (update-in {:a {:b 1}} [:a :b] inc) => {:a {:b 2}}"
  (if (= (length keys) 1)
    (update coll (first keys) f)
    (put coll (first keys)
         (update-in (get coll (first keys)) (rest keys) f))))

(defn update [coll key f]
  "Apply f to the value at key, returning the modified collection.
   (update {:count 5} :count inc) => {:count 6}
   (update [10 20 30] 1 inc) => [10 21 30]
   Errors if key does not exist."
  (if (or (array? coll) (bytes? coll) (string? coll))
    (when (or (< key 0) (>= key (length coll)))
      (error {:error :key-error
              :reason :index-out-of-bounds :key key :message (string "index out of bounds: " key)}))
    (unless (has? coll key)
      (error {:error :key-error
              :reason :key-not-found :key key :message (string "key not found: " key)})))
  (put coll key (f (get coll key))))

(defn sum [xs]
  "Sum a sequence of numbers. (sum [1 2 3]) => 6"
  (fold + 0 xs))

(defn product [xs]
  "Product of a sequence of numbers. (product [1 2 3]) => 6"
  (fold * 1 xs))


## ── Module export closure ───────────────────────────────────────────
## Last expression: a closure returning a struct of all exports.
## Called by init_stdlib to register stdlib functions as primitives.

(fn []
  {:map map :filter filter :fold fold :reduce reduce :keep keep
   :identity identity :complement complement :constantly constantly
   :compose compose :comp comp :partial partial :juxt juxt
   :all? all? :any? any? :find find :find-index find-index
   :count count :nth nth :zip zip :flatten flatten
   :take-while take-while :drop-while drop-while :distinct distinct
   :frequencies frequencies :mapcat mapcat :group-by group-by
   :map-indexed map-indexed :partition partition :interpose interpose
   :min-key min-key :max-key max-key :memoize memoize :sort-by sort-by
   :time/stopwatch time/stopwatch :time/elapsed time/elapsed
   :call-count call-count :global? global? :fiber/self fiber/self
   :fiber/new? fiber/new? :fiber/alive? fiber/alive? :fiber/paused? fiber/paused?
   :fiber/dead? fiber/dead? :fiber/error? fiber/error?
   :new? fiber/new? :alive? fiber/alive? :paused? fiber/paused?
   :dead? fiber/dead? :error? fiber/error?
   :fn/cfg fn/cfg :fn/cfg-label fn/cfg-label
   :fn/cfg-dot fn/cfg-dot :fn/cfg-mermaid fn/cfg-mermaid
   :*stdin* *stdin* :*stdout* *stdout* :*stderr* *stderr*
    :print print :println println :eprint eprint :eprintln eprintln
    :*spawn* *spawn* :*scheduler* *scheduler* :*io-backend* *io-backend*
     :ev/spawn ev/spawn :make-async-scheduler make-async-scheduler
     :ev/run ev/run :ev/step ev/step :ev/with-scheduler ev/with-scheduler
     :ev/join ev/join :ev/join-protected ev/join-protected
     :ev/abort ev/abort :ev/as-completed ev/as-completed
     :ev/select ev/select :ev/race ev/race
     :ev/timeout ev/timeout :ev/scope ev/scope
     :ev/map ev/map :ev/map-limited ev/map-limited
     :ev/shutdown ev/shutdown :*shutdown* *shutdown*
     :ev/futex-wait ev/futex-wait :ev/futex-wake ev/futex-wake
     :merge merge :inc inc :dec dec
     :stream/for-each stream/for-each :stream/fold stream/fold
     :stream/collect stream/collect :stream/into-array stream/into-array
     :stream/map stream/map :stream/filter stream/filter
     :stream/take stream/take :stream/drop stream/drop
     :stream/concat stream/concat :stream/zip stream/zip
     :stream/pipe stream/pipe
      :stream/read-all (fn [port] (port/read-all port))
      :port/lines port/lines :port/chunks port/chunks :port/writer port/writer
        :subprocess/system subprocess/system
        :sort-with sort-with :sort-by-cmp sort-by-cmp
        :ffi/pin ffi/pin
        :from-pairs from-pairs :sum sum :product product
        :update update :get-in get-in :put-in put-in :update-in update-in})
