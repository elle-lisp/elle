(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-member-cascade-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because an over-free in this shape
# SIGSEGVs under --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into
# one shared process where a segfault would take the whole harness down. It is
# exercised by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_capture_cell_member_cascade_uaf`).
#
# WHAT IT PINS — the drop-time external-reference rescue
# (docs/impl/region/ownership.md § "The incoming edge table and the
# external-reference rescue") on the cascade / stored-member shape. A server
# fiber reads a request off a socket, stores a MEMBER of the parsed request
# (`(get req :params)`, inline in `req`'s region) into a module-level
# `@`-capture cell, then reads a SIBLING member (`(get req :id)`) inside a
# `protect` sub-fiber to frame the reply. The sibling read makes `req` a capture
# of the protect fiber's closure, so `req`'s region is capture-adopted into that
# closure's Owned subtree and dies with its subtree drop when the fiber
# completes — while the cell still references the `:params` member inside it.
# The cell's store recorded a content edge into `req`'s region
# (`capture_store_with_rebind`), so the drop rescues the region to the RC
# baseline instead of tearing it down: the final read of the cell below sees the
# live member, and the region frees at the cell's release.
#
# THE SHAPE'S INGREDIENTS (each is load-bearing for reaching the rescue path):
#   * the cell stores a MEMBER of `req` (`(get req :params)`), not the whole
#     value — storing `req` itself keeps the parent live on its own count;
#   * a SIBLING member of the SAME `req` is read AFTER the store (`(get req :id)`
#     into the JSON reply) — that capture is what adopts `req` into the protect
#     fiber's closure;
#   * the value ARRIVES OVER A SOCKET into a spawned fiber and the reply is
#     WRITTEN BACK to it — an in-process literal / json/parse of a literal string
#     gives `req` a program-lifetime region the drop never reaches.
#
# ORIGIN — mu's lib/cont/ipc.lisp: each JSON-RPC driver callback (on-retire,
# on-owner-summarize, on-tool-action, …) does exactly this — `(assign got-X params)`
# of the RPC request's params member, while the same dispatch reads the request's
# id/method to frame the reply (the ipc, ipc-roundtrip, spawn-agent, and
# adopt-grant suites). Distinct from the direct-free capture-cell string shape
# (`region_capture_cell_string_accum_uaf`); this is the cascade / stored-member
# path.

(def @captured nil)

(defn handle-connection [conn]
  # Mirrors lib/cont/ipc.lisp handle-connection + a driver callback: read a
  # request, store its :params MEMBER in the cell, then read a SIBLING member
  # (:id) to frame the reply written back over the connection.
  (let [line (port/read-line conn)
        req (json/parse line :keys :keyword)]
    (assign captured (get req :params {}))
    (protect (begin
               (port/write conn
                           (string (json/serialize {:jsonrpc "2.0"
                                   :id (get req :id)
                                   :result "ack"}) "\n"))
               (port/flush conn)))
    (protect (port/close conn))))

# Short basename: unix sun_path is capped at 108 bytes (see tests/elle/net.lisp).
(with-temp-dir d
               (let [sock (path/join d "u.sock")
                     listener (unix/listen sock)]
                 (ev/spawn (fn []
                             (protect (handle-connection (unix/accept listener)))))
                 (ev/sleep 0.03)
                 (let [s (unix/connect sock)]
                   (port/write s
                               (string (json/serialize {:jsonrpc "2.0"
                                       :id 1
                                       :method "retire"
                                       :params {:done true :who "child"}}) "\n"))
                   (port/flush s)
                   (port/read-line s)
                   (protect (port/close s)))
                 (ev/sleep 0.06)))

# The witness read: the rescued member is live after the connection fiber's
# subtree drop, so this prints the stored value rather than dereferencing a
# freed page.
(println (string "region-capture-cell-member-cascade-uaf: captured="
                 (get captured :done nil)))
