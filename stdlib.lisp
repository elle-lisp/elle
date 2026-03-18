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

(def map (fn (f coll)
  (cond
    ((or (array? coll) (array? coll) (bytes? coll) (bytes? coll))
     (letrec ((loop (fn (i acc)
                      (if (>= i (length coll))
                        (reverse acc)
                        (loop (+ i 1) (cons (f (get coll i)) acc))))))
       (loop 0 ())))
    ((or (string? coll) (string? coll))
     (letrec ((loop (fn (i acc)
                      (if (>= i (length coll))
                        (reverse acc)
                         (loop (+ i 1) (cons (f (get coll i)) acc))))))
       (loop 0 ())))
    ((or (pair? coll) (empty? coll))
     (if (empty? coll)
       ()
       (cons (f (first coll)) (map f (rest coll)))))
      ((set? coll)
       (letrec ((items (set->array coll))
                 (len (length items))
                 (is-immutable (= (type-of coll) :set))
                 (loop (fn (i acc)
                         (if (= i len)
                           acc
                           (loop (+ i 1) (add acc (f (get items i))))))))
          (if is-immutable
            (loop 0 (set))
            (loop 0 (@set)))))

    (true (error {:error :type-error :message "map: not a sequence"})))))


(def filter (fn (p lst)
  (if (empty? lst)
    ()
    (if (p (first lst))
      (cons (first lst) (filter p (rest lst)))
      (filter p (rest lst))))))

(def fold (fn (f init lst)
  (if (empty? lst)
    init
    (fold f (f init (first lst)) (rest lst)))))

(def reduce fold)
(def keep filter)

## ── Functional combinators ──────────────────────────────────────────

(def identity (fn (x) x))

(def complement (fn (f)
  (fn (& args) (not (f ;args)))))

(def constantly (fn (x)
  (fn (& _) x)))

(def compose (fn (& fns)
  (fold (fn (composed f)
          (fn (& args) (composed (f ;args))))
        identity
        fns)))

(def comp compose)

(def partial (fn (f & bound)
  (fn (& args) (f ;bound ;args))))

(def juxt (fn (& fns)
  (fn (& args)
    (map (fn (f) (f ;args)) fns))))

## ── Collection search & predicates ──────────────────────────────────

(def all? (fn (pred coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (if (empty? coll)
       true
       (if (pred (first coll))
         (all? pred (rest coll))
         false)))
    ((or (array? coll) (array? coll))
     (letrec ((loop (fn (i)
                      (if (>= i (length coll))
                        true
                        (if (pred (get coll i))
                          (loop (+ i 1))
                          false)))))
       (loop 0)))
    (true (error {:error :type-error :message "all?: not a sequence"})))))

(def any? (fn (pred coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (if (empty? coll)
       false
       (if (pred (first coll))
         true
         (any? pred (rest coll)))))
    ((or (array? coll) (array? coll))
     (letrec ((loop (fn (i)
                      (if (>= i (length coll))
                        false
                        (if (pred (get coll i))
                          true
                          (loop (+ i 1)))))))
       (loop 0)))
    (true (error {:error :type-error :message "any?: not a sequence"})))))

(def find (fn (pred coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (if (empty? coll)
       nil
       (if (pred (first coll))
         (first coll)
         (find pred (rest coll)))))
    ((or (array? coll) (array? coll))
     (letrec ((loop (fn (i)
                      (if (>= i (length coll))
                        nil
                        (if (pred (get coll i))
                          (get coll i)
                          (loop (+ i 1)))))))
       (loop 0)))
    (true (error {:error :type-error :message "find: not a sequence"})))))

(def find-index (fn (pred coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (letrec ((go (fn (i l)
                    (if (empty? l)
                      nil
                      (if (pred (first l))
                        i
                        (go (+ i 1) (rest l)))))))
       (go 0 coll)))
    ((or (array? coll) (array? coll))
     (letrec ((loop (fn (i)
                      (if (>= i (length coll))
                        nil
                        (if (pred (get coll i))
                          i
                          (loop (+ i 1)))))))
       (loop 0)))
    (true (error {:error :type-error :message "find-index: not a sequence"})))))

(def count (fn (pred coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (fold (fn (n x) (if (pred x) (+ n 1) n)) 0 coll))
    ((or (array? coll) (array? coll))
     (letrec ((loop (fn (i n)
                      (if (>= i (length coll))
                        n
                        (loop (+ i 1) (if (pred (get coll i)) (+ n 1) n))))))
       (loop 0 0)))
    (true (error {:error :type-error :message "count: not a sequence"})))))

(def nth (fn (n coll)
  (get coll n)))

## ── Collection transforms ───────────────────────────────────────────

(def zip (fn (& colls)
  (letrec
    ((to-list (fn (c)
       (cond
         ((or (pair? c) (empty? c)) c)
         ((or (array? c) (array? c))
          (letrec ((loop (fn (i acc)
                           (if (>= i (length c))
                             (reverse acc)
                             (loop (+ i 1) (cons (get c i) acc))))))
            (loop 0 ())))
         (true (error {:error :type-error :message "zip: not a sequence"})))))
     (from-list (fn (lst orig)
       (cond
         ((or (pair? orig) (empty? orig)) lst)
         ((array? orig)
          (let ((arr @[]))
            (each x in lst (push arr x))
            arr))
         ((array? orig) (apply array lst)))))
     (zip-lists (fn (lists)
       (if (any? empty? lists)
         ()
         (cons (map first lists)
               (zip-lists (map rest lists)))))))
    (if (empty? colls)
      ()
      (let* ((lists (map to-list colls))
             (result (zip-lists lists)))
        (from-list result (first colls)))))))

(def flatten (fn (coll)
  (letrec
    ((to-list (fn (c)
       (letrec ((loop (fn (i acc)
                        (if (>= i (length c))
                          (reverse acc)
                          (loop (+ i 1) (cons (get c i) acc))))))
         (loop 0 ()))))
     (flat (fn (lst)
       (if (empty? lst)
         ()
         (let ((x (first lst)))
           (cond
             ((pair? x)
              (append (flat x) (flat (rest lst))))
             ((or (array? x) (array? x))
              (append (flat (to-list x)) (flat (rest lst))))
             (true
              (cons x (flat (rest lst))))))))))
    (cond
      ((or (pair? coll) (empty? coll)) (flat coll))
      ((array? coll)
       (let ((result @[]))
         (each x in (flat (to-list coll)) (push result x))
         result))
      ((array? coll)
       (apply array (flat (to-list coll))))
      (true (error {:error :type-error :message "flatten: not a sequence"}))))))

(def take-while (fn (pred coll)
  (letrec
    ((tw-list (fn (lst)
       (if (empty? lst)
         ()
         (if (pred (first lst))
           (cons (first lst) (tw-list (rest lst)))
           ())))))
    (cond
      ((or (pair? coll) (empty? coll)) (tw-list coll))
      ((array? coll)
       (let ((result @[]))
         (letrec ((loop (fn (i)
                          (when (< i (length coll))
                            (let ((x (get coll i)))
                              (when (pred x)
                                (push result x)
                                (loop (+ i 1))))))))
           (loop 0))
         result))
      ((array? coll)
       (let ((lst (tw-list (letrec ((loop (fn (i acc)
                                            (if (>= i (length coll))
                                              (reverse acc)
                                              (loop (+ i 1) (cons (get coll i) acc))))))
                             (loop 0 ())))))
         (apply array lst)))
      (true (error {:error :type-error :message "take-while: not a sequence"}))))))

(def drop-while (fn (pred coll)
  (letrec
    ((dw-list (fn (lst)
       (if (empty? lst)
         ()
         (if (pred (first lst))
           (dw-list (rest lst))
           lst)))))
    (cond
      ((or (pair? coll) (empty? coll)) (dw-list coll))
      ((array? coll)
       (letrec ((find-start (fn (i)
                              (if (>= i (length coll))
                                (length coll)
                                (if (pred (get coll i))
                                  (find-start (+ i 1))
                                  i)))))
         (let ((start (find-start 0))
               (result @[]))
           (letrec ((loop (fn (i)
                            (when (< i (length coll))
                              (push result (get coll i))
                              (loop (+ i 1))))))
             (loop start))
           result)))
      ((array? coll)
       (let ((lst (dw-list (letrec ((loop (fn (i acc)
                                            (if (>= i (length coll))
                                              (reverse acc)
                                              (loop (+ i 1) (cons (get coll i) acc))))))
                             (loop 0 ())))))
         (apply array lst)))
      (true (error {:error :type-error :message "drop-while: not a sequence"}))))))

(def distinct (fn (coll)
  (let ((seen @{}))
    (letrec
      ((dist-list (fn (lst)
         (if (empty? lst)
           ()
           (if (has? seen (first lst))
             (dist-list (rest lst))
             (begin (put seen (first lst) true)
                    (cons (first lst) (dist-list (rest lst)))))))))
      (cond
        ((or (pair? coll) (empty? coll)) (dist-list coll))
        ((array? coll)
         (let ((result @[]))
           (each x in coll
             (unless (has? seen x)
               (put seen x true)
               (push result x)))
           result))
        ((array? coll)
         (let ((lst (dist-list (letrec ((loop (fn (i acc)
                                               (if (>= i (length coll))
                                                 (reverse acc)
                                                 (loop (+ i 1) (cons (get coll i) acc))))))
                                (loop 0 ())))))
           (apply array lst)))
        (true (error {:error :type-error :message "distinct: not a sequence"})))))))

(def frequencies (fn (coll)
  (let ((counts @{}))
    (each x in coll
      (put counts x (+ 1 (if (has? counts x) (get counts x) 0))))
    (freeze counts))))

(def mapcat (fn (f coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (fold (fn (acc x) (append acc (f x))) () coll))
    ((array? coll)
     (let ((result @[]))
       (each x in coll
         (each y in (f x) (push result y)))
       result))
    ((array? coll)
     (apply array (fold (fn (acc x) (append acc (f x))) ()
                        (letrec ((loop (fn (i acc)
                                         (if (>= i (length coll))
                                           (reverse acc)
                                           (loop (+ i 1) (cons (get coll i) acc))))))
                          (loop 0 ())))))
    (true (error {:error :type-error :message "mapcat: not a sequence"})))))

(def group-by (fn (f coll)
  (let ((groups @{}))
    (each x in coll
      (let ((k (f x)))
        (if (has? groups k)
          (push (get groups k) x)
          (put groups k @[x]))))
    groups)))

(def map-indexed (fn (f coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (letrec ((go (fn (i l)
                    (if (empty? l)
                      ()
                      (cons (f i (first l)) (go (+ i 1) (rest l)))))))
       (go 0 coll)))
    ((array? coll)
     (let ((result @[]))
       (letrec ((loop (fn (i)
                        (when (< i (length coll))
                          (push result (f i (get coll i)))
                          (loop (+ i 1))))))
         (loop 0))
       result))
    ((array? coll)
     (apply array
       (letrec ((go (fn (i)
                      (if (>= i (length coll))
                        ()
                        (cons (f i (get coll i)) (go (+ i 1)))))))
         (go 0))))
    (true (error {:error :type-error :message "map-indexed: not a sequence"})))))

(def partition (fn (n coll)
  (cond
    ((or (pair? coll) (empty? coll))
     (if (or (<= n 0) (empty? coll))
       ()
       (cons (take n coll)
             (partition n (drop n coll)))))
    ((array? coll)
     (let ((result @[]))
       (letrec ((loop (fn (i)
                        (when (< i (length coll))
                          (let ((chunk @[]))
                            (letrec ((inner (fn (j)
                                             (when (and (< j (+ i n)) (< j (length coll)))
                                               (push chunk (get coll j))
                                               (inner (+ j 1))))))
                              (inner i))
                            (push result chunk)
                            (loop (+ i n)))))))
         (loop 0))
       result))
    ((array? coll)
     (letrec ((to-list (fn (c)
                          (letrec ((loop (fn (i acc)
                                          (if (>= i (length c))
                                            (reverse acc)
                                            (loop (+ i 1) (cons (get c i) acc))))))
                            (loop 0 ()))))
              (part (fn (lst)
                      (if (or (<= n 0) (empty? lst))
                        ()
                        (cons (apply array (take n lst))
                              (part (drop n lst)))))))
       (apply array (part (to-list coll)))))
    (true (error {:error :type-error :message "partition: not a sequence"})))))

(def interpose (fn (sep coll)
  (letrec
    ((ip-list (fn (lst)
       (if (or (empty? lst) (empty? (rest lst)))
         lst
         (cons (first lst)
               (cons sep (ip-list (rest lst))))))))
    (cond
      ((or (pair? coll) (empty? coll)) (ip-list coll))
      ((array? coll)
       (if (< (length coll) 2)
         coll
         (let ((result @[(get coll 0)]))
           (letrec ((loop (fn (i)
                            (when (< i (length coll))
                              (push result sep)
                              (push result (get coll i))
                              (loop (+ i 1))))))
             (loop 1))
           result)))
      ((array? coll)
       (let ((lst (ip-list (letrec ((loop (fn (i acc)
                                           (if (>= i (length coll))
                                             (reverse acc)
                                             (loop (+ i 1) (cons (get coll i) acc))))))
                             (loop 0 ())))))
         (apply array lst)))
      (true (error {:error :type-error :message "interpose: not a sequence"}))))))

(def min-key (fn (f & args)
  (fold (fn (best x) (if (< (f x) (f best)) x best))
        (first args)
        (rest args))))

(def max-key (fn (f & args)
  (fold (fn (best x) (if (> (f x) (f best)) x best))
        (first args)
        (rest args))))

(def memoize (fn (f)
  (let ((cache @{}))
    (fn (& args)
      (let ((key (if (= (length args) 1) (first args) (string args))))
        (if (has? cache key)
          (get cache key)
          (let ((result (f ;args)))
            (put cache key result)
            result)))))))

(def sort-by (fn (keyfn coll)
  (letrec
    ((to-list (fn (c)
       (cond
         ((or (pair? c) (empty? c)) c)
         ((or (array? c) (array? c))
          (letrec ((loop (fn (i acc)
                           (if (>= i (length c))
                             (reverse acc)
                             (loop (+ i 1) (cons (get c i) acc))))))
            (loop 0 ())))
         (true (error {:error :type-error :message "sort-by: not a sequence"})))))
     (from-list (fn (lst orig)
       (cond
         ((or (pair? orig) (empty? orig)) lst)
         ((array? orig)
          (let ((arr @[]))
            (each x in lst (push arr x))
            arr))
         ((array? orig) (apply array lst)))))
     (merge (fn (a b)
       (cond
         ((empty? a) b)
         ((empty? b) a)
         ((<= (first (first a)) (first (first b)))
          (cons (first a) (merge (rest a) b)))
         (true
          (cons (first b) (merge a (rest b)))))))
     (halve (fn (lst)
       (let ((mid (/ (length lst) 2)))
         [(take mid lst) (drop mid lst)])))
     (msort (fn (lst)
       (if (or (empty? lst) (empty? (rest lst)))
         lst
         (let (([left right] (halve lst)))
           (merge (msort left) (msort right)))))))
    (let* ((as-list (to-list coll))
           (decorated (map (fn (x) (list (keyfn x) x)) as-list))
           (sorted (msort decorated))
           (result (map (fn (pair) (first (rest pair))) sorted)))
      (from-list result coll)))))

(def sort-with (fn (cmp coll)
  (letrec
    ((to-list (fn (c)
       (cond
         ((or (pair? c) (empty? c)) c)
         ((array? c)
          (letrec ((loop (fn (i acc)
                           (if (>= i (length c))
                             (reverse acc)
                             (loop (+ i 1) (cons (get c i) acc))))))
            (loop 0 ())))
         (true (error {:error :type-error :message "sort-with: not a sequence"})))))
     (from-list (fn (lst orig)
       (cond
         ((or (pair? orig) (empty? orig)) lst)
         ((mutable? orig)
          (let ((arr @[]))
            (each x in lst (push arr x))
            arr))
         ((array? orig) (apply array lst)))))
     (merge-lists (fn (a b)
       (cond
         ((empty? a) b)
         ((empty? b) a)
         ((<= (cmp (first a) (first b)) 0)
          (cons (first a) (merge-lists (rest a) b)))
         (true
          (cons (first b) (merge-lists a (rest b)))))))
     (halve (fn (lst)
       (let ((mid (/ (length lst) 2)))
         [(take mid lst) (drop mid lst)])))
     (msort (fn (lst)
       (if (or (empty? lst) (empty? (rest lst)))
         lst
         (let (([left right] (halve lst)))
           (merge-lists (msort left) (msort right)))))))
    (from-list (msort (to-list coll)) coll))))

(def sort-by-cmp sort-with)

## ── Time utilities ──────────────────────────────────────────────────

(def time/stopwatch (fn ()
  (coro/new (fn ()
    (let ((start (clock/monotonic)))
      (while true
        (yield (- (clock/monotonic) start))))))))

(def time/elapsed (fn (thunk)
  (let ((start (clock/monotonic)))
    (let ((result (thunk)))
      (list result (- (clock/monotonic) start))))))

## ── VM query wrappers ───────────────────────────────────────────────

(def call-count (fn (f) (vm/query "call-count" f)))
(def global? (fn (sym) (vm/query "global?" sym)))
(def fiber/self (fn () (vm/query "fiber/self" nil)))

## ── Arena introspection ─────────────────────────────────────────────



## ── Control flow graph rendering ────────────────────────────────────

(defn fn/cfg (target & opts)
  "Render a closure or fiber's control flow graph as text.
   Optional format keyword: :mermaid (default) or :dot.
   (fn/cfg my-fn)          => Mermaid flowchart string
   (fn/cfg my-fn :dot)     => DOT digraph string
   (fn/cfg my-fn :mermaid) => Mermaid flowchart string"
  (let* ((fmt (if (empty? opts)
                :mermaid
                (if (> (length opts) 1)
                  (error {:error :arity-error :message "fn/cfg: expected at most 1 format keyword"})
                  (first opts))))
         (cfg (fn/flow target)))
    (when (nil? cfg)
      (error {:error :type-error :message "fn/cfg: target has no LIR"}))
    (cond
      ((= fmt :mermaid) (fn/cfg-mermaid cfg))
      ((= fmt :dot)     (fn/cfg-dot cfg))
      (true (error {:error :type-error :message (-> "fn/cfg: unknown format "
                                   (append (string fmt))
                                   (append ", expected :mermaid or :dot"))})))))

(defn fn/cfg-label (cfg)
  "Build the label string from a CFG struct's metadata."
  (let* ((name (get cfg :name))
         (doc (get cfg :doc)))
    (if (nil? name)
      (if (nil? doc) "anonymous" doc)
      name)))

(defn fn/cfg-dot (cfg)
  "Render a CFG struct as a DOT digraph string with compact instructions."
  (letrec ((dot-escape (fn (s)
             (-> s
               (string/replace "\"" "\\\"")
               (string/replace "{" "\\{")
               (string/replace "}" "\\}")
               (string/replace "|" "\\|")
               (string/replace "<" "\\<")
               (string/replace ">" "\\>")))))
    (let ((result (-> "digraph {\n  label=\""
                    (append (dot-escape (string/replace (fn/cfg-label cfg) "\n" " ")))
                    (append " arity:")
                    (append (get cfg :arity))
                    (append " regs:")
                    (append (string (get cfg :regs)))
                    (append " locals:")
                    (append (string (get cfg :locals)))
                    (append "\";\n  node [shape=record fontname=\"monospace\" fontsize=10];\n"))))
      (each block (get cfg :blocks)
        (let* ((lbl (string (get block :label)))
               (display (get block :annotated))
               (term-display (get block :term-display))
               (term-kind (get block :term-kind))
               (edges (get block :edges))
               (color (cond
                        ((= term-kind :return) "#4444cc")
                        ((= term-kind :branch) "#cc8800")
                        ((= term-kind :yield)  "#008844")
                        (true                  "#444444"))))
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
  (letrec ((mmd-escape (fn (s)
             (-> s
               (string/replace "&" "&amp;")
               (string/replace "\"" "&quot;")))))
    (let ((result (-> "flowchart TD\n"
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
                    (append "  classDef normal fill:#f8f9fa,stroke:#6c757d\n"))))
      (each block (get cfg :blocks)
        (let* ((lbl (string (get block :label)))
               (display (get block :display))
               (term-display (get block :term-display))
               (term-kind (get block :term-kind))
               (edges (get block :edges))
               # Choose node shape based on terminator kind
               # All labels are quoted to avoid parser issues with special chars
               (open-delim (cond
                             ((= term-kind :branch) "{\"")
                             ((= term-kind :return) "([\"")
                             ((= term-kind :yield)  "{{\"")
                             (true                  "[\"")))
               (close-delim (cond
                              ((= term-kind :branch) "\"}")
                              ((= term-kind :return) "\"])")
                              ((= term-kind :yield)  "\"}}")
                              (true                  "\"]")))
               # Build node content with compact instructions
               (content (-> (append "block" lbl)
                          (append "<br/>"))))
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
          (let ((cls (cond
                       ((= lbl (string (get cfg :entry))) "entry")
                       ((= term-kind :return)  "ret")
                       ((= term-kind :branch)  "branch")
                       ((= term-kind :yield)   "yield_block")
                       (true                   "normal"))))
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

(def merge (fn (a b)
  "Merge struct b into struct a. Both must be structs of the same mutability.
   Keys in b override keys in a. Returns the same mutability as the inputs.
   Signals :type-error if either argument is not a struct or mutabilities differ."
  (if (not (struct? a))
    (error {:error :type-error :message "merge: first argument must be a struct"})
    (if (not (struct? b))
      (error {:error :type-error :message "merge: second argument must be a struct"})
      (if (not (= (mutable? a) (mutable? b)))
        (error {:error :type-error :message "merge: mutability mismatch — both arguments must be the same mutability"})
        (let ((result (@struct)))
          (each k in (keys a) (put result k (get a k)))
          (each k in (keys b) (put result k (get b k)))
          (if (mutable? a) result (freeze result))))))))

## ── Stream combinators ─────────────────────────────────────────────
##
## Streams are coroutines. A read stream yields values when resumed.
## Sink combinators consume a stream to completion and return a result.
## Transform combinators return a new coroutine wrapping a source.
## Port-to-stream converters return coroutines backed by an open port.
##
## All port-backed streams must be consumed inside a scheduler context
## (ev/spawn or ev/run) because port I/O emits SIG_IO.

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
  (var acc init)
  (coro/resume source)
  (while (not (coro/done? source))
    (assign acc (f acc (coro/value source)))
    (coro/resume source))
  acc)

(defn stream/collect [source]
  "Collect all values yielded by source into a list.
   Builds in reverse using cons then reverses — O(n).
   Signal: errors only (no user callback)."
  (var acc ())
  (coro/resume source)
  (while (not (coro/done? source))
    (assign acc (cons (coro/value source) acc))
    (coro/resume source))
  (reverse acc))

(defn stream/into-array [source]
  "Collect all values yielded by source into a mutable array.
   Signal: errors only (no user callback)."
  (let [[result @[]]]
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
    (var remaining n)
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
    (var skipped 0)
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
      (var done false)
      (let [[vals (map (fn [s]
                        (coro/resume s)
                        (when (coro/done? s) (assign done true))
                        (coro/value s))
                      sources)]]
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
   Must be called inside a scheduler context (ev/spawn or sync-scheduler)."
  (coro/new (fn []
    (forever
      (let [[line (stream/read-line port)]]
        (if (nil? line)
          (begin (port/close port) (break))
          (yield line)))))))

(defn port/chunks [port size]
  "Yields byte chunks of `size` from port. Final chunk may be smaller.
   Must be called inside a scheduler context."
  (coro/new (fn []
    (forever
      (let [[chunk (stream/read port size)]]
        (if (nil? chunk)
          (begin (port/close port) (break))
          (yield chunk)))))))

(defn port/writer [port]
  "Returns a write-stream coroutine. Resume with a string to write it.
   Resume with nil to close the port. Must be called inside a scheduler context."
  (coro/new (fn []
    (forever
      (let [[val (yield nil)]]
        (if (nil? val)
          (begin (port/close port) (break))
          (stream/write port val)))))))

## ── Standard port parameters ────────────────────────────────────────

(def *stdin*  (parameter (port/stdin)))
(def *stdout* (parameter (port/stdout)))
(def *stderr* (parameter (port/stderr)))

## ── Scheduler ───────────────────────────────────────────────────────

(def sync-scheduler
  (fn [fiber]
    "Run a fiber to completion, dispatching I/O requests synchronously."
    (let ((backend (io/backend :sync)))
      (fiber/resume fiber)
      (forever
       (case (fiber/status fiber)
         :dead      (break (fiber/value fiber))
         :error     (fiber/propagate fiber)
         :paused (cond
                       ((not (= 0 (bit/and (fiber/bits fiber) 1)))
                        (fiber/propagate fiber))
                       ((not (= 0 (bit/and (fiber/bits fiber) 512)))
                        (fiber/resume fiber (io/execute backend (fiber/value fiber))))
                       (true
                        (fiber/resume fiber))))))))

(def *scheduler* (parameter sync-scheduler))

(def ev/spawn
  (fn [closure]
    "Spawn a closure in a new fiber managed by the current scheduler."
    (let ((fiber (fiber/new closure (bit/or 1 512 2048))))
      ((*scheduler*) fiber))))

## ── Async scheduler ─────────────────────────────────────────────────

(defn make-async-scheduler ()
  "Create an async scheduler. Returns (scheduler-fn pump-fn shutdown-fn).
   scheduler-fn: (fn [fiber]) — registers fiber for async execution.
   pump-fn: (fn []) — pumps event loop until all fibers complete.
   shutdown-fn: (fn [timeout-ms]) — signal shutdown; pump-fn executes it."
  (let ((backend       (io/backend :async))
        (runnable      @[])
        (pending       @{})
        (shutdown-req  @[nil]))  # nil = running, integer = shutdown requested with timeout

    (defn handle-fiber-after-resume [fiber]
      "Route a fiber to the right place after resume."
      (case (fiber/status fiber)
        :dead   nil
        :error  (fiber/propagate fiber)
        :paused (cond
                  ((not (= 0 (bit/and (fiber/bits fiber) 1)))
                   (fiber/propagate fiber))
                  ((not (= 0 (bit/and (fiber/bits fiber) 512)))
                   (let ((id (io/submit backend (fiber/value fiber))))
                     (put pending id fiber)))
                  (true
                   (push runnable fiber)))))

    (defn drain-runnable []
      "Run all runnable fibers."
      (while (> (length runnable) 0)
        (let ((fiber (pop runnable)))
          (fiber/resume fiber)
          (handle-fiber-after-resume fiber))))

    (defn process-completions []
      "Wait for I/O completions and route fibers."
      (let ((completions (io/wait backend (- 0 1))))
        (each c in completions
          (let* ((id    (get c :id))
                 (fiber (get pending id)))
            (when (not (nil? fiber))
              (del pending id)
              (if (nil? (get c :error))
                (begin
                  (fiber/resume fiber (get c :value))
                  (handle-fiber-after-resume fiber))
                (error (get c :error))))))))

    (defn do-shutdown [timeout-ms]
      "Abort all pending fibers, pump for timeout-ms, cancel stragglers."
      # Phase 1: abort all pending fibers (inject error, let defer run).
      # Cancel the io_uring SQE for each so the kernel stops waiting.
      (each [id fiber] in (pairs (freeze pending))
        (del pending id)
        (io/cancel backend id)
        (let [[[ok? _] (protect (fiber/abort fiber {:error :shutdown}))]]
          (when ok? (handle-fiber-after-resume fiber))))
      # Phase 2: drain cancel CQEs and let aborted fibers unwind
      (when (> timeout-ms 0)
        (let [[deadline (+ (clock/monotonic) (/ timeout-ms 1000.0))]]
          (while (and (> (+ (length runnable) (length pending)) 0)
                      (< (clock/monotonic) deadline))
            (drain-runnable)
            (when (> (length pending) 0)
              (let [[completions (io/wait backend 10)]]
                (each c in completions
                  (let* [[id    (get c :id)]
                         [fiber (get pending id)]]
                    (when (not (nil? fiber))
                      (del pending id)
                      (when (nil? (get c :error))
                        (fiber/resume fiber (get c :value))
                        (handle-fiber-after-resume fiber))))))))))
      # Phase 3: cancel any stragglers and their pending I/O
      (each [id fiber] in (pairs (freeze pending))
        (del pending id)
        (io/cancel backend id)
        (protect (fiber/cancel fiber {:error :shutdown})))
      (while (> (length runnable) 0)
        (let [[fiber (pop runnable)]]
          (protect (fiber/cancel fiber {:error :shutdown})))))

    (list
      # scheduler-fn: register fiber
      (fn (fiber)
        (push runnable fiber)
        fiber)
      # pump-fn: event loop
      (fn ()
        (block :loop
          (forever
            (drain-runnable)
            (when (= (length pending) 0)
              (break :loop nil))
            # Check for shutdown request
            (let [[timeout (get shutdown-req 0)]]
              (unless (nil? timeout)
                (do-shutdown timeout)
                (break :loop nil)))
            (process-completions))))
      # shutdown-fn: signal shutdown
      (fn (timeout-ms)
        (put shutdown-req 0 timeout-ms)))))

(def *shutdown* (make-parameter nil))

(defn ev/shutdown [& args]
  "Shut down the current event loop. Optional timeout-ms (default 0) gives
   fibers time to clean up before being hard-killed."
  (let [[timeout-ms (or (first args) 0)]
        [shutdown-fn (*shutdown*)]]
    (when (nil? shutdown-fn)
      (error {:error :error :message "ev/shutdown: not inside an event loop"}))
    (shutdown-fn timeout-ms)))

(defn ev/run (& thunks)
  "Run thunks concurrently with async I/O.
   Creates an async scheduler, spawns each thunk as a fiber, pumps until done."
  (let (((scheduler-fn pump-fn shutdown-fn) (make-async-scheduler)))
    (parameterize ((*scheduler* scheduler-fn)
                   (*shutdown* shutdown-fn))
      (each t in thunks
        (ev/spawn t))
      (pump-fn))))

(defn inc [x]
  "Return x + 1."
  (+ x 1))

(defn dec [x]
  "Return x - 1."
  (- x 1))

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
  (let* ((exec-opts (if (empty? opts)
                       {:stdin :null}
                       (merge {:stdin :null} (freeze (first opts)))))
         (proc         (subprocess/exec program args exec-opts))
         # Drain pipes BEFORE subprocess/wait (deadlock invariant — see docstring).
         (stdout-bytes (if (nil? (get proc :stdout))
                         (bytes)
                         (stream/read-all (get proc :stdout))))
         (stderr-bytes (if (nil? (get proc :stderr))
                         (bytes)
                         (stream/read-all (get proc :stderr))))
         (exit-code    (subprocess/wait proc)))
    (when (not (nil? (get proc :stdout))) (port/close (get proc :stdout)))
    (when (not (nil? (get proc :stderr))) (port/close (get proc :stderr)))
    {:exit   exit-code
     :stdout (string stdout-bytes)
     :stderr (string stderr-bytes)}))

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
   :fn/cfg fn/cfg :fn/cfg-label fn/cfg-label
   :fn/cfg-dot fn/cfg-dot :fn/cfg-mermaid fn/cfg-mermaid
   :*stdin* *stdin* :*stdout* *stdout* :*stderr* *stderr*
    :sync-scheduler sync-scheduler :*scheduler* *scheduler*
     :ev/spawn ev/spawn :make-async-scheduler make-async-scheduler
     :ev/run ev/run :ev/shutdown ev/shutdown :*shutdown* *shutdown*
     :merge merge :inc inc :dec dec
     :stream/for-each stream/for-each :stream/fold stream/fold
     :stream/collect stream/collect :stream/into-array stream/into-array
     :stream/map stream/map :stream/filter stream/filter
     :stream/take stream/take :stream/drop stream/drop
     :stream/concat stream/concat :stream/zip stream/zip
     :stream/pipe stream/pipe
      :port/lines port/lines :port/chunks port/chunks :port/writer port/writer
        :subprocess/system subprocess/system
        :sort-with sort-with :sort-by-cmp sort-by-cmp})
