(elle/epoch 12)
# An error that a `defer` catches, runs cleanup for, and re-propagates, after
# an unrelated earlier error was caught and handled.
#
# `tests/integration/error_reporting.rs` locates the two raising forms by the
# `:first` and `:second` keywords, so keep each of them on one line of its own.

(defn caught-raise []
  (error {:error :first :message "caught and handled"}))

(defn uncaught-raise []
  (error {:error :second :message "reaches the root through defer"}))

(protect (caught-raise))

(defer
  (println "cleanup ran")
  (uncaught-raise))
