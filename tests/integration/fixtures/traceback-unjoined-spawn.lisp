(elle/epoch 12)
# An error raised in a spawned fiber that nothing joins. The scheduler catches
# it, and surfaces it when the program's own work finishes.
#
# `tests/integration/error_reporting.rs` locates the raising form by the
# `:orphan` keyword, so keep it on one line of its own.

(defn uncaught-raise []
  (error {:error :orphan :message "raised in an unjoined fiber"}))

(ev/spawn (fn [] (uncaught-raise)))

(ev/sleep 0.05)
