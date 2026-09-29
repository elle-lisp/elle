(elle/epoch 12)
## Synchronisation objects must not grow the symbol table.
##
## A futex key only has to be a unique hashable value; it does not have to
## be a symbol. `sys/unique` mints process-unique integers, and the sync
## constructors key their futexes with it, so building locks in a loop
## retains nothing. Keying with `(gensym)` instead would intern one symbol
## per constructor call, permanently — the assertions below read
## `debug/symbol-count` across a constructor loop and demand it flat.

(def sync ((import "std/sync")))

# Distinctness: the uniqueness gensym provided, without the interning.
(assert (not (= (sys/unique) (sys/unique))) "sys/unique mints distinct keys")
(assert (int? (sys/unique)) "sys/unique mints integers")

# Warm each shape first so one-time interning (module load, first-call
# paths) settles before the measured window.
(defn measure [f]
  (var i 0)
  (while (< i 20)
    (f)
    (assign i (+ i 1)))
  (let [before (debug/symbol-count)]
    (var j 0)
    (while (< j 100)
      (f)
      (assign j (+ j 1)))
    (- (debug/symbol-count) before)))

(assert (= 0 (measure (fn [] (sync:make-futex false))))
        "make-futex must not intern symbols")
(assert (= 0 (measure (fn [] (sync:make-lock))))
        "make-lock must not intern symbols")
(assert (= 0 (measure (fn [] (sync:make-queue 4))))
        "make-queue must not intern symbols")
(assert (= 0 (measure (fn [] (sync:make-monitor))))
        "make-monitor must not intern symbols")

(println "sync-keys: ok")
