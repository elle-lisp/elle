## Elle standard library
##
## Loaded at startup after primitives are registered.
## Unlike the prelude (which is macro-only), these define
## runtime functions that need the full pipeline.

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

    (true (error [:type-error "map: not a sequence"])))))


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
    (true (error [:type-error "all?: not a sequence"])))))

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
    (true (error [:type-error "any?: not a sequence"])))))

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
    (true (error [:type-error "find: not a sequence"])))))

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
    (true (error [:type-error "find-index: not a sequence"])))))

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
    (true (error [:type-error "count: not a sequence"])))))

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
         (true (error [:type-error "zip: not a sequence"])))))
     (from-list (fn (lst orig)
       (cond
         ((or (pair? orig) (empty? orig)) lst)
         ((array? orig)
          (let ((arr @[]))
            (each x in lst (push arr x))
            arr))
         ((array? orig) (apply tuple lst)))))
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
       (apply tuple (flat (to-list coll))))
      (true (error [:type-error "flatten: not a sequence"]))))))

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
         (apply tuple lst)))
      (true (error [:type-error "take-while: not a sequence"]))))))

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
         (apply tuple lst)))
      (true (error [:type-error "drop-while: not a sequence"]))))))

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
           (apply tuple lst)))
        (true (error [:type-error "distinct: not a sequence"])))))))

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
     (apply tuple (fold (fn (acc x) (append acc (f x))) ()
                        (letrec ((loop (fn (i acc)
                                         (if (>= i (length coll))
                                           (reverse acc)
                                           (loop (+ i 1) (cons (get coll i) acc))))))
                          (loop 0 ())))))
    (true (error [:type-error "mapcat: not a sequence"])))))

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
     (apply tuple
       (letrec ((go (fn (i)
                      (if (>= i (length coll))
                        ()
                        (cons (f i (get coll i)) (go (+ i 1)))))))
         (go 0))))
    (true (error [:type-error "map-indexed: not a sequence"])))))

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
                        (cons (apply tuple (take n lst))
                              (part (drop n lst)))))))
       (apply tuple (part (to-list coll)))))
    (true (error [:type-error "partition: not a sequence"])))))

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
         (apply tuple lst)))
      (true (error [:type-error "interpose: not a sequence"]))))))

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
         (true (error [:type-error "sort-by: not a sequence"])))))
     (from-list (fn (lst orig)
       (cond
         ((or (pair? orig) (empty? orig)) lst)
         ((array? orig)
          (let ((arr @[]))
            (each x in lst (push arr x))
            arr))
         ((array? orig) (apply tuple lst)))))
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
                  (error [:arity-error "fn/cfg: expected at most 1 format keyword"])
                  (first opts))))
         (cfg (fn/flow target)))
    (when (nil? cfg)
      (error [:type-error "fn/cfg: target has no LIR"]))
    (cond
      ((= fmt :mermaid) (fn/cfg-mermaid cfg))
      ((= fmt :dot)     (fn/cfg-dot cfg))
      (true (error [:type-error (-> "fn/cfg: unknown format "
                                  (append (string fmt))
                                  (append ", expected :mermaid or :dot"))])))))

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
    (let ((fiber (fiber/new closure (bit/or 1 512))))
      ((*scheduler*) fiber))))

## ── Async scheduler ─────────────────────────────────────────────────

(defn make-async-scheduler ()
  "Create an async scheduler. Returns (scheduler-fn pump-fn).
   scheduler-fn: (fn [fiber]) — registers fiber for async execution.
   pump-fn: (fn []) — pumps event loop until all fibers complete."
  (let ((backend  (io/backend :async))
        (runnable @[])
        (pending  @{}))
    (list
      # scheduler-fn: register fiber
      (fn (fiber)
        (push runnable fiber)
        fiber)
      # pump-fn: event loop
      (fn ()
        (block :loop
          (forever
            # 1. Run all runnable fibers
            (while (> (length runnable) 0)
              (let ((fiber (pop runnable)))
                (fiber/resume fiber)
                 (case (fiber/status fiber)
                   :dead      nil
                   :error     (fiber/propagate fiber)
                   :paused (cond
                               ((not (= 0 (bit/and (fiber/bits fiber) 1)))
                                (fiber/propagate fiber))
                               ((not (= 0 (bit/and (fiber/bits fiber) 512)))
                                (let ((id (io/submit backend (fiber/value fiber))))
                                  (put pending id fiber)))
                               (true
                                (push runnable fiber))))))
            # 2. If nothing pending, done
            (when (= (length pending) 0)
              (break :loop nil))
            # 3. Wait for completions
            (let ((completions (io/wait backend (- 0 1))))
              (each c in completions
                (let* ((id    (get c :id))
                       (fiber (get pending id)))
                  (when (not (nil? fiber))
                    (del pending id)
                    (if (nil? (get c :error))
                      (begin
                        (fiber/resume fiber (get c :value))
                         (case (fiber/status fiber)
                           :dead      nil
                           :error     (fiber/propagate fiber)
                           :paused (cond
                                       ((not (= 0 (bit/and (fiber/bits fiber) 1)))
                                        (fiber/propagate fiber))
                                       ((not (= 0 (bit/and (fiber/bits fiber) 512)))
                                        (let ((id2 (io/submit backend (fiber/value fiber))))
                                          (put pending id2 fiber)))
                                        (true
                                         (push runnable fiber)))))
                      (error (get c :error)))))))))))))


(defn ev/run (& thunks)
  "Run thunks concurrently with async I/O.
   Creates an async scheduler, spawns each thunk as a fiber, pumps until done."
  (let (((scheduler-fn pump-fn) (make-async-scheduler)))
    (parameterize ((*scheduler* scheduler-fn))
      (each t in thunks
        (ev/spawn t))
      (pump-fn))))

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
   :arena/allocs arena/allocs
   :fn/cfg fn/cfg :fn/cfg-label fn/cfg-label
   :fn/cfg-dot fn/cfg-dot :fn/cfg-mermaid fn/cfg-mermaid
   :*stdin* *stdin* :*stdout* *stdout* :*stderr* *stderr*
   :sync-scheduler sync-scheduler :*scheduler* *scheduler*
   :ev/spawn ev/spawn :make-async-scheduler make-async-scheduler
   :ev/run ev/run})
