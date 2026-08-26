(elle/epoch 12)
# The simplest uncaught error: nothing catches anything first.
#
# `tests/integration/error_reporting.rs` locates the raising form by the
# `:only` keyword, so keep it on one line of its own.

(defn uncaught-raise []
  (error {:error :only :message "reaches the root"}))

(uncaught-raise)
