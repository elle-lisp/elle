(elle/epoch 10)
## Pre-prelude definitions
##
## Compiled and executed before the prelude loads.
## Only raw special forms and %-prefixed primitives are available.
## Provides functions that prelude macros need at expansion time.

(def last
  (fn [coll]
    (if (%eq (length coll) 0)
      (emit :error {:error :argument-error :message "last: empty sequence"})
      (get coll (%sub (length coll) 1)))))

(def butlast
  (fn [coll]
    (let [n (length coll)]
      (if (%eq n 0) (slice coll 0 0) (slice coll 0 (%sub n 1))))))

(fn [] {:last last :butlast butlast})
