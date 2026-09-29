(elle/epoch 13)
# audited: 2026-09-29
# Reads the page-claims tool's arguments, and plans the commands one run makes.
# docs/impl/region/colocation.md

(defn usage-error [message]
  (error {:error :usage-error
          :message (string "page-claims: " message
                           " (usage: [--depth N] [--top N] [--elle PATH] ELLE-ARGS…)")}))

(defn at-least-one [option text]
  "TEXT as the positive integer OPTION takes."
  (when (nil? text) (usage-error (string option " takes a value")))
  (let [[ok? n] (protect (parse-int text))]
    (unless (and ok? (>= n 1))
      (usage-error (string option " takes a positive integer, not " text)))
    n))

(defn options [args]
  "Read ARGS, what `sys/args` answers. elle hands the program the `--` that
  ended its own flags, so a leading one is dropped. The options end at the
  first argument that is not one, and the rest is the profiled elle's. Answers
  {:depth :top :elle :args}."
  (def given (->array args))
  (var i (if (and (> (length given) 0) (= (get given 0) "--")) 1 0))
  (def opts @{:depth 3 :top 20 :elle "target/profiling/elle"})
  (forever
    (def arg (get given i))
    (cond
      (= arg "--depth")
        (put opts :depth (at-least-one arg (get given (+ i 1))))
      (= arg "--top")
        (put opts :top (at-least-one arg (get given (+ i 1))))
      (= arg "--elle")
        (let [path (get given (+ i 1))]
          (when (nil? path) (usage-error "--elle takes a value"))
          (put opts :elle path))
      (break nil))
    (assign i (+ i 2)))
  {:depth (get opts :depth)
   :top (get opts :top)
   :elle (get opts :elle)
   :args (slice given i (length given))})

(defn commands [opts dir]
  "The files and commands of one run in DIR, a directory of its own. The
  profiled elle reads a stdlib cache in DIR, and the warm-up fills it first by
  running an empty program outside callgrind (docs/impl/region/colocation.md)."
  (def cache (string "--cache=" (path/join dir "cache")))
  (def out (path/join dir "callgrind.out"))
  (def empty (path/join dir "empty.lisp"))
  {:out out
   :empty empty
   :warm [(get opts :elle) cache empty]
   :profile (concat ["valgrind" "--tool=callgrind" "--dump-instr=no"
                     "--separate-callers12=*add_page*"
                     (string "--callgrind-out-file=" out) (get opts :elle) cache]
                    (->array (get opts :args)))})

(fn [] {:options options :commands commands})
