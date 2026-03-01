## Elle standard library
##
## Loaded at startup after primitives are registered.
## Unlike the prelude (which is macro-only), these define
## runtime functions that need the full pipeline.

## ── Higher-order functions ──────────────────────────────────────────

(def map (fn (f coll)
  (cond
    ((or (array? coll) (tuple? coll) (bytes? coll) (blob? coll))
     (letrec ((loop (fn (i acc)
                      (if (>= i (length coll))
                        (reverse acc)
                        (loop (+ i 1) (cons (f (get coll i)) acc))))))
       (loop 0 ())))
    ((or (string? coll) (buffer? coll))
     (letrec ((loop (fn (i acc)
                      (if (>= i (length coll))
                        (reverse acc)
                        (loop (+ i 1) (cons (f (string/char-at coll i)) acc))))))
       (loop 0 ())))
    ((or (pair? coll) (empty? coll))
     (if (empty? coll)
       ()
       (cons (f (first coll)) (map f (rest coll)))))
    (true (error :type-error "map: not a sequence")))))

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

## ── Graph visualization ─────────────────────────────────────────────

(defn fn/dot-escape (s)
  "Escape special DOT record-label characters."
  (-> s
    (string/replace "{" "\\{")
    (string/replace "}" "\\}")
    (string/replace "|" "\\|")
    (string/replace "<" "\\<")
    (string/replace ">" "\\>")))

(defn fn/graph (cfg)
  "Convert a fn/flow CFG struct to DOT format string."
  (let* ((name (get cfg :name))
         (doc (get cfg :doc))
         (label (if (nil? name)
                  (if (nil? doc) "anonymous" doc)
                  name))
         (result (-> "digraph {\n  label=\""
                   (append label)
                   (append " arity:")
                   (append (get cfg :arity))
                   (append " regs:")
                   (append (string (get cfg :regs)))
                   (append " locals:")
                   (append (string (get cfg :locals)))
                   (append "\";\n  node [shape=record];\n"))))
    (each block (get cfg :blocks)
      (let* ((lbl (string (get block :label)))
             (instrs (get block :instrs))
             (term (get block :term))
             (edges (get block :edges)))
        (set result (-> result
                      (append "  block")
                      (append lbl)
                      (append " [label=\"{block")
                      (append lbl)))
        (set result (append result "|"))
        (each instr instrs
          (set result (-> result
                        (append (fn/dot-escape instr))
                        (append "\\l"))))
        (set result (-> result
                      (append "|")
                      (append (fn/dot-escape term))
                      (append "}\"];\n")))
        (each edge edges
          (set result (-> result
                        (append "  block")
                        (append lbl)
                        (append " -> block")
                        (append (string edge))
                        (append ";\n"))))))
    (append result "}\n")))

(defn fn/save-graph (closure path)
  "Save the LIR control flow graph of a closure as a DOT file."
  (file/write path (fn/graph (fn/flow closure))))
