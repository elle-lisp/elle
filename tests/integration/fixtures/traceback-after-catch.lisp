(elle/epoch 12)
# A program that catches one error, then raises a different, uncaught one.
#
# `tests/integration/error_reporting.rs` locates the two raising forms by the
# `:first` and `:second` keywords, so keep each of them on one line of its own.

(defn caught-raise []
  (error {:error :first :message "caught and handled"}))

(defn uncaught-raise []
  (error {:error :second :message "reaches the root"}))

(protect (caught-raise))

(uncaught-raise)
